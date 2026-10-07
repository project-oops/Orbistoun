//! The command processor's own memory work, carried out at submit.
//!
//! Part of a submitted stream is work the command processor does against memory with no shader
//! involved: `DMA_DATA` fills and copies, `RELEASE_MEM` end-of-pipe writes, `WAIT_REG_MEM` checks.
//! That work is reproduced exactly on the CPU. The walk goes in order and stops at the first packet
//! that needs the GPU, so a fence is written only when the work before it ran (D705). At the first
//! draw of each segment - the draws between two pieces of memory work - [`CpMemory::run_draws`]
//! may carry out that segment's draws together (D729). Register writes and cache control
//! are passed over: nothing here executes register state, and there is no cache before guest
//! memory. Field layouts are the public PM4 ones, cited from the collection's Mesa tree where used.

use crate::packet::{self, PacketKind, build::measured};

/// `PKT3_WAIT_REG_MEM` (`sid.h:90`). No measured builder emits it; the open-toolchain GL context
/// writes it by hand to the public layout.
pub const WAIT_REG_MEM: u8 = 0x3c;

/// `PKT3_WAIT_REG_MEM64` (`amd_cp_packets_gfx11.h:1764`): `WAIT_REG_MEM` over a 64-bit value -
/// function and space, the address (8-byte aligned), a 64-bit reference and a 64-bit mask, then
/// the poll interval (`:1765-1826`). `sceAgcDcbWaitUntilSafeForRendering` writes it on a flip
/// label (`-5a17`).
pub const WAIT_REG_MEM64: u8 = 0x93;

/// `PKT3_CLEAR_STATE` (`sid.h:41`): resets register state to its defaults. radeonsi's preamble
/// emits it with one zero body word (`si_state.c:4880-4881`); it touches no memory.
pub const CLEAR_STATE: u8 = 0x12;
/// `PKT3_CONTEXT_CONTROL` (`sid.h:64`): two words, load enables then shadow enables
/// (`cp_pm4_table_data_gfx11.json:1219`).
pub const CONTEXT_CONTROL: u8 = 0x28;
/// The shadow enables of `CONTEXT_CONTROL`'s second word - `shadow_global_config` (0),
/// `shadow_per_context_state` (1), `shadow_global_uconfig` (15), `shadow_gfx_sh_regs` (16) and
/// `shadow_cs_sh_regs` (24) (`cp_pm4_table_data_gfx11.json:1275-1309`). With any set, the command
/// processor writes register state to a shadow in memory, which is not carried out here. The first
/// word's load enables select what later `LOAD_*_REG` packets read, and touch no memory themselves.
const CONTEXT_CONTROL_SHADOW_ENABLES: u32 = (1 << 0) | (1 << 1) | (1 << 15) | (1 << 16) | (1 << 24);
/// `update_shadow_enables`, bit 31 of the second word (`cp_pm4_table_data_gfx11.json:1315`): the
/// shadow enables are applied only with it set, so without it the word changes nothing.
const CONTEXT_CONTROL_UPDATE_SHADOW_ENABLES: u32 = 1 << 31;

/// `PKT3_WRITE_DATA` (`sid.h:83`): a control word, a destination, then the data words
/// (`cp_pm4_table_data_gfx11.json:6677`).
pub const WRITE_DATA: u8 = 0x37;
/// `WRITE_DATA` `DST_SEL` (control bits 11:8, `sid.h:84`) values naming memory: `1`, memory
/// synchronised across GRBM (`pkt3.json:26`), `2`, through L2, and `5`, memory
/// (`cp_pm4_table_data_gfx11.json:6680-6696`). The same memory here; Mesa's IB parser treats these
/// three, and only these, as memory addresses (`parse_cp_pm4_table_data_json.py:77-79`). `0` is a
/// memory-mapped register.
const WRITE_DATA_DST_SEL_MEMORY: [u32; 3] = [1, 2, 5];
/// `WRITE_DATA` `DST_SEL` 0, `mem_mapped_register` (`cp_pm4_table_data_gfx11.json:6680`): the
/// destination is a register, not memory.
const WRITE_DATA_DST_SEL_REGISTER: u32 = 0;
/// `WRITE_DATA` `ADDR_INCR` (control bit 16): set, every word goes to the same address.
const WRITE_DATA_NO_INCREMENT: u32 = 1 << 16;

/// `DMA_DATA` `SRC_SEL` values (`sid.h:178`, bits 30:29): the source is the packet's own data word.
const SRC_SEL_DATA: u32 = 2;
/// `DMA_DATA` `SRC_SEL`: the source is an address - direct, or through L2 (`sid.h:178`). Both are
/// the same memory here.
const SRC_SEL_ADDRESS: [u32; 2] = [0, 3];
/// `DMA_DATA` `DST_SEL` (bits 21:20): an address, direct or through L2. `1` is GDS, not memory.
const DST_SEL_ADDRESS: [u32; 2] = [0, 3];
/// `CP_DMA_ME_COMMAND.BYTE_COUNT`, bits 0-25 on this generation (`gfx103.json:12266`).
const BYTE_COUNT_MASK: u32 = 0x03ff_ffff;
/// `CP_DMA_ME_COMMAND` `SAIC` (bit 28): hold the source address rather than increment it
/// (`no_increment`, `cp_pm4_table_data_gfx11.json`), so every destination dword is the same source
/// dword.
const SOURCE_HOLD: u32 = 1 << 28;
/// `CP_DMA_ME_COMMAND` `DAIC` (bit 29): hold the destination address. Not reproduced; a packet
/// asking for it stops execution.
const DESTINATION_HOLD: u32 = 1 << 29;
/// `CP_DMA_ME_COMMAND` `SAS` (bit 26): the source address is in register space - meaningful only
/// with `SRC_SEL` 0 (`src_addr_using_sas`). Not reproduced there.
const SOURCE_IN_REGISTERS: u32 = 1 << 26;

/// `RELEASE_MEM` `DATA_SEL` (`sid.h:163-167`, bits 31:29 of body dword one).
const DATA_SEL_DISCARD: u32 = 0;
const DATA_SEL_VALUE_32BIT: u32 = 1;
const DATA_SEL_VALUE_64BIT: u32 = 2;
const DATA_SEL_TIMESTAMP: u32 = 3;

/// `WAIT_REG_MEM` compare functions (`sid.h:91-93`) and its memory-space bit (`sid.h:94`).
const WAIT_EQUAL: u32 = 3;
const WAIT_NOT_EQUAL: u32 = 4;
const WAIT_GREATER_OR_EQUAL: u32 = 5;
const WAIT_MEM_SPACE: u32 = 1 << 4;

/// The memory the command processor works on, and the clock it stamps with.
pub trait CpMemory {
    /// `length` bytes at `address`, or `None` when the range is not readable guest memory.
    fn read(&self, address: u64, length: usize) -> Option<Vec<u8>>;
    /// Writes `bytes` at `address`, or answers `false` when the range is not writable guest memory.
    fn write(&mut self, address: u64, bytes: &[u8]) -> bool;
    /// The 64-bit GPU clock counter a `RELEASE_MEM` with `DATA_SEL` 3 writes.
    fn timestamp(&mut self) -> u64;
    /// A `RELEASE_MEM` stored its value at `address` and raised the interrupt with context id
    /// `context` - what the display acts on for a flip queued in the stream (D728). The default
    /// has no display to tell.
    fn released(&mut self, _address: u64, _context: u32) {}
    /// Whether the memory at `address` holds exactly `expected`, asked of a whole colour target to
    /// see whether anything wrote it. The default reads a copy; guest memory compares in place.
    fn holds(&self, address: u64, expected: &[u8]) -> bool {
        self.read(address, expected.len())
            .is_some_and(|bytes| bytes == expected)
    }
    /// Fills `count` bytes at `address` with a four-byte pattern, or answers `false` when the range
    /// is not writable guest memory. The default builds the bytes and writes them; guest memory
    /// fills in place.
    fn fill(&mut self, address: u64, pattern: u32, count: usize) -> bool {
        let mut bytes = pattern.to_le_bytes().repeat(count.div_ceil(4));
        bytes.truncate(count);
        self.write(address, &bytes)
    }
    /// Copies `count` bytes from `source` to `destination`, or answers `false` when either range is
    /// not guest memory it may use. The default reads a copy and writes it; guest memory copies in
    /// place.
    fn copy(&mut self, source: u64, destination: u64, count: usize) -> bool {
        self.read(source, count)
            .is_some_and(|bytes| self.write(destination, &bytes))
    }
    /// Runs `edit` over the `count` little-endian words at `address` and keeps the result, or
    /// answers `false` with nothing changed when the range is not writable guest memory. The
    /// default reads a copy and writes it back; guest memory edits in place, so a frame tiles
    /// straight into its target.
    fn edit_words(&mut self, address: u64, count: usize, edit: &mut dyn FnMut(&mut [u32])) -> bool {
        let Some(bytes) = self.read(address, count * 4) else {
            return false;
        };
        let mut words: Vec<u32> = bytes
            .chunks_exact(4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
            .collect();
        edit(&mut words);
        let mut out = vec![0u8; bytes.len()];
        for (to, word) in out.chunks_exact_mut(4).zip(&words) {
            to.copy_from_slice(&word.to_le_bytes());
        }
        self.write(address, &out)
    }
    /// Carries out one segment of the stream's draws together and writes what they drew into guest
    /// memory, answering whether that happened (D712, D729). Asked once per segment, at its first
    /// draw, after the memory work before it and before the memory work after it; no memory work
    /// sits between a segment's draws, so running them as one equals running them in turn. The
    /// default carries out nothing, which leaves the draw as a stop.
    fn run_draws(&mut self, _segment: &DrawSegment<'_>) -> bool {
        false
    }
    /// Carries out one compute dispatch, its writes landing in guest memory, answering whether
    /// that happened exactly. The default carries out nothing, which leaves the dispatch as a
    /// stop.
    fn run_dispatch(&mut self, _dispatch: &Dispatch<'_>) -> bool {
        false
    }
}

/// A run of a stream's draws with no memory work between them, carried out together (D729).
///
/// Every register write in the stream still reaches the draws through its own packet, so a
/// segment's register state is the stream's up to its last draw.
#[derive(Debug, Clone, Copy)]
pub struct DrawSegment<'a> {
    /// The whole stream.
    pub stream: &'a [u8],
    /// Byte offset of the segment's first draw packet.
    pub first: u32,
    /// Byte offset just past its last draw packet.
    pub end: u32,
    /// Whether it is all the stream's GPU work: every draw, no memory work between any two of
    /// them, and no dispatch anywhere - so the submission as prepared is its draws and nothing
    /// the command processor carries out itself.
    pub whole: bool,
}

/// A compute dispatch as the stream asks for it: where it is, so the registers set before it can
/// be read from the stream, its grid, and its dispatch initiator.
#[derive(Debug, Clone, Copy)]
pub struct Dispatch<'a> {
    /// The whole stream, whose register writes before [`Self::offset`] are the dispatch's state.
    pub stream: &'a [u8],
    /// Byte offset of the `DISPATCH_DIRECT` packet.
    pub offset: u32,
    /// Groups in x, y and z.
    pub groups: [u32; 3],
    /// `COMPUTE_DISPATCH_INITIATOR`.
    pub initiator: u32,
}

/// `DISPATCH_DIRECT`'s body: the grid then the initiator (`sid.h:43`; the measured builder in
/// `packet::build::dispatch_direct`).
fn dispatch_direct(
    stream: &[u8],
    offset: u32,
    body: &[u32],
    memory: &mut dyn CpMemory,
    result: &mut CpExecution,
) -> Result<(), Stop> {
    let [x, y, z, initiator, ..] = *body else {
        return Err(Stop::Malformed);
    };
    let dispatch = Dispatch {
        stream,
        offset,
        groups: [x, y, z],
        initiator,
    };
    if !memory.run_dispatch(&dispatch) {
        return Err(Stop::NeedsGpu);
    }
    result.dispatches += 1;
    Ok(())
}

/// The draw packets a submission's draws are carried out for together: `DRAW_INDEX_2` and
/// `DRAW_INDEX_AUTO`, the two the translator turns into draws. An indirect draw reads its arguments
/// from memory at execution time and is not among them.
pub(crate) const fn is_draw(opcode: u8) -> bool {
    matches!(
        opcode,
        measured::DRAW_INDEX_2 | measured::DRAW_INDEX_AUTO | DRAW_INDEX_OFFSET_2
    )
}

/// `DRAW_INDEX_OFFSET_2` (Mesa `sid.h` `0x35`): an indexed draw from the bound index buffer.
const DRAW_INDEX_OFFSET_2: u8 = 0x35;

/// Why execution stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stopped {
    /// Every packet was carried out or had no effect on memory.
    #[default]
    Completed,
    /// A packet needs the GPU - a draw, a dispatch, or a command not known to be memory-only.
    NeedsGpu {
        /// Byte offset of the packet in the stream.
        offset: u32,
        /// Its opcode.
        opcode: u8,
    },
    /// A packet named memory the guest cannot read or write.
    OutOfBounds {
        /// Byte offset of the packet in the stream.
        offset: u32,
        /// Its opcode.
        opcode: u8,
        /// The guest address it named.
        address: u64,
    },
    /// A `WAIT_REG_MEM` whose condition does not hold. Nothing else runs concurrently here, so it
    /// never will: the command processor would wait forever, and so nothing after it retires.
    WaitNeverSatisfied {
        /// Byte offset of the packet in the stream.
        offset: u32,
    },
    /// The stream could not be walked with confidence, or a packet was shorter than its layout.
    Malformed {
        /// Byte offset of the packet in the stream.
        offset: u32,
    },
}

/// What one submission's command-processor work did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CpExecution {
    /// `DMA_DATA` fills carried out.
    pub fills: usize,
    /// `DMA_DATA` copies carried out.
    pub copies: usize,
    /// `RELEASE_MEM` writes carried out (a discarded one counts - it was reached and retired).
    pub releases: usize,
    /// `WAIT_REG_MEM` checks that held.
    pub waits: usize,
    /// Draw packets carried out, through [`CpMemory::run_draws`], one segment at a time.
    pub draws: usize,
    /// Compute dispatches carried out, through [`CpMemory::run_dispatch`].
    pub dispatches: usize,
    /// Bytes written to guest memory.
    pub bytes_written: u64,
    /// Where it stopped.
    pub stopped: Stopped,
}

/// Opcodes with no effect on memory: register state, no-ops, and cache control.
fn is_memory_inert(opcode: u8, body_dwords: usize) -> bool {
    matches!(
        opcode,
        measured::NOP
            | measured::SET_BASE
            | measured::INDEX_BUFFER_SIZE
            | measured::INDEX_BASE
            | measured::NUM_INSTANCES
            | measured::ACQUIRE_MEM
            | measured::SET_CONTEXT_REG
            | measured::SET_SH_REG
            | measured::SET_UCONFIG_REG
            | measured::SET_UCONFIG_REG_INDEX
            | measured::SET_CONTEXT_REG_INDIRECT
            | measured::SET_SH_REG_INDIRECT
            | measured::SET_UCONFIG_REG_INDIRECT
            | measured::STALL_COMMAND_BUFFER_PARSER
            | CLEAR_STATE
    ) || (opcode == measured::EVENT_WRITE && body_dwords == 1)
}

/// Carries out a stream's command-processor memory work, in order, stopping at the first packet it
/// cannot carry out exactly.
pub fn execute(stream: &[u8], memory: &mut dyn CpMemory) -> CpExecution {
    let mut result = CpExecution::default();
    let walked = packet::walk(stream);
    // A walk that desynchronised or overran may have read a body as a header: executing any of it
    // could fill memory from bytes that were never a fill. Nothing runs.
    if !walked.is_trustworthy() {
        result.stopped = Stopped::Malformed { offset: 0 };
        return result;
    }
    // The draw segments, found before any draw runs; each is carried out at its first draw.
    let segments = draw_segments(stream, &walked.packets);
    let mut next_segment = segments.iter().peekable();
    for packet in &walked.packets {
        let offset = packet.offset;
        let opcode = match packet.kind {
            PacketKind::RegisterWrite { .. } | PacketKind::Filler => continue,
            PacketKind::Reserved => {
                result.stopped = Stopped::Malformed { offset };
                return result;
            }
            PacketKind::Command { opcode } => opcode,
        };
        let start = packet.body_offset() as usize;
        let end = start + packet.body_length() as usize;
        let bytes = stream.get(start..end).unwrap_or_default();
        // Decided from the length alone, before any body is built: a GL frame is tens of thousands
        // of packets, nearly all inert or draws.
        if is_memory_inert(opcode, bytes.len() / 4) {
            continue;
        }
        if is_draw(opcode) {
            if let Some(segment) = next_segment.next_if(|segment| segment.first == offset)
                && !memory.run_draws(segment)
            {
                result.stopped = Stopped::NeedsGpu { offset, opcode };
                return result;
            }
            result.draws += 1;
            continue;
        }
        let body: Vec<u32> = bytes
            .chunks_exact(4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
            .collect();
        let step = match opcode {
            measured::DMA_DATA => dma_data(&body, memory, &mut result),
            measured::RELEASE_MEM => crate::perf::span(crate::perf::Span::OtherMemory, || {
                release_mem(&body, memory, &mut result)
            }),
            WAIT_REG_MEM => crate::perf::span(crate::perf::Span::OtherMemory, || {
                wait_reg_mem(&body, memory, &mut result)
            }),
            WAIT_REG_MEM64 => crate::perf::span(crate::perf::Span::OtherMemory, || {
                wait_reg_mem64(&body, memory, &mut result)
            }),
            CONTEXT_CONTROL => context_control(&body),
            packet::build::DISPATCH_DIRECT => {
                dispatch_direct(stream, offset, &body, memory, &mut result)
            }
            WRITE_DATA => crate::perf::span(crate::perf::Span::OtherMemory, || {
                write_data(&body, memory, &mut result)
            }),
            _ => Err(Stop::NeedsGpu),
        };
        if let Err(stop) = step {
            result.stopped = match stop {
                Stop::NeedsGpu => Stopped::NeedsGpu { offset, opcode },
                Stop::OutOfBounds(address) => Stopped::OutOfBounds {
                    offset,
                    opcode,
                    address,
                },
                Stop::Wait => Stopped::WaitNeverSatisfied { offset },
                Stop::Malformed => Stopped::Malformed { offset },
            };
            return result;
        }
    }
    result
}

/// The stream's draws split at every packet between two of them that is neither a draw nor
/// memory-inert (D729): each segment runs from a draw to the last draw before the next such packet.
fn draw_segments<'a>(stream: &'a [u8], packets: &[packet::Packet]) -> Vec<DrawSegment<'a>> {
    let mut segments: Vec<DrawSegment<'a>> = Vec::new();
    let mut open = false;
    for p in packets {
        let PacketKind::Command { opcode } = p.kind else {
            continue;
        };
        if is_draw(opcode) {
            let end = p.body_offset() + p.body_length();
            match segments.last_mut() {
                Some(segment) if open => segment.end = end,
                _ => segments.push(DrawSegment {
                    stream,
                    first: p.offset,
                    end,
                    whole: false,
                }),
            }
            open = true;
        } else if !is_memory_inert(opcode, p.body_length() as usize / 4) {
            open = false;
        }
    }
    let dispatches = packets.iter().any(|p| {
        matches!(p.kind, PacketKind::Command { opcode } if opcode == packet::build::DISPATCH_DIRECT)
    });
    if let [only] = segments.as_mut_slice() {
        only.whole = !dispatches;
    }
    segments
}

enum Stop {
    NeedsGpu,
    /// The address a packet named that is not the guest's.
    OutOfBounds(u64),
    Wait,
    Malformed,
}

const fn address(low: u32, high: u32) -> u64 {
    ((high as u64) << 32) | low as u64
}

/// A `DMA_DATA` packet's operands, as this carries them out.
struct DmaData {
    src_sel: u32,
    /// The source word: the pattern of a fill, or the low half of a copy's source address.
    src_low: u32,
    src_high: u32,
    destination: u64,
    count: usize,
    /// `SAIC`: every destination dword is the source's first.
    source_held: bool,
}

/// `DMA_DATA`: word zero, source (data or address), destination address, command
/// (`sid.h:177-184`); the byte count is 26 bits on this generation (`gfx103.json:12266`).
fn decode_dma_data(body: &[u32]) -> Result<DmaData, Stop> {
    let [control, src_low, src_high, dst_low, dst_high, command, ..] = *body else {
        return Err(Stop::Malformed);
    };
    let dst_sel = (control >> 20) & 0x3;
    let src_sel = (control >> 29) & 0x3;
    let register_source = src_sel == 0 && command & SOURCE_IN_REGISTERS != 0;
    if !DST_SEL_ADDRESS.contains(&dst_sel) || command & DESTINATION_HOLD != 0 || register_source {
        return Err(Stop::NeedsGpu);
    }
    Ok(DmaData {
        src_sel,
        src_low,
        src_high,
        destination: address(dst_low, dst_high),
        count: (command & BYTE_COUNT_MASK) as usize,
        source_held: command & SOURCE_HOLD != 0,
    })
}

/// A `DMA_DATA` fill of memory with one repeated word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fill {
    /// First byte written.
    pub destination: u64,
    /// The word repeated from the destination on.
    pub pattern: u32,
    /// Bytes written.
    pub count: u64,
}

/// The fill a `DMA_DATA` body describes, or `None` for a copy or a packet this does not carry out.
#[must_use]
pub fn fill_of(body: &[u32]) -> Option<Fill> {
    let dma = decode_dma_data(body).ok()?;
    (dma.src_sel == SRC_SEL_DATA).then_some(Fill {
        destination: dma.destination,
        pattern: dma.src_low,
        count: dma.count as u64,
    })
}

/// `DMA_DATA`'s `DST_SEL` "nowhere" (`cp_pm4_table_data_gfx11.json:1979`, `dst_nowhere` = 2): the
/// source is read into L2 and nothing is written - radeonsi's shader prefetch
/// (`si_state_draw.cpp:659-664`).
const DST_SEL_NOWHERE: u32 = 2;

/// A prefetch writes nothing; the only thing it can do to the stream is fault on a source that is
/// not memory, so an unreadable source is refused.
fn dma_prefetch(body: &[u32], memory: &dyn CpMemory) -> Option<Result<(), Stop>> {
    let [control, src_low, src_high, _, _, command, ..] = *body else {
        return None;
    };
    if (control >> 20) & 0x3 != DST_SEL_NOWHERE {
        return None;
    }
    if !SRC_SEL_ADDRESS.contains(&((control >> 29) & 0x3)) {
        return Some(Err(Stop::NeedsGpu));
    }
    let count = (command & BYTE_COUNT_MASK) as usize;
    let source = address(src_low, src_high);
    let readable = count == 0 || memory.read(source, count).is_some();
    Some(if readable {
        Ok(())
    } else {
        Err(Stop::OutOfBounds(source))
    })
}

fn dma_data(body: &[u32], memory: &mut dyn CpMemory, result: &mut CpExecution) -> Result<(), Stop> {
    if let Some(prefetched) = dma_prefetch(body, memory) {
        return prefetched;
    }
    let DmaData {
        src_sel,
        src_low,
        src_high,
        destination,
        count,
        source_held,
    } = decode_dma_data(body)?;
    // In place: a GL frame fills and copies megabytes per submission.
    let done = if count == 0 {
        true
    } else if src_sel == SRC_SEL_DATA {
        crate::perf::span(crate::perf::Span::OtherMemory, || {
            memory.fill(destination, src_low, count)
        })
    } else if SRC_SEL_ADDRESS.contains(&src_sel) && source_held {
        // The held source's first dword, repeated across the destination.
        let word = memory
            .read(address(src_low, src_high), 4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]));
        word.is_some_and(|word| memory.fill(destination, word, count))
    } else if SRC_SEL_ADDRESS.contains(&src_sel) {
        crate::perf::span(crate::perf::Span::Copy, || {
            memory.copy(address(src_low, src_high), destination, count)
        })
    } else {
        return Err(Stop::NeedsGpu);
    };
    if !done {
        // The copy's source where it was the one not there, else its destination.
        let source = address(src_low, src_high);
        let missing = if SRC_SEL_ADDRESS.contains(&src_sel) && memory.read(source, count).is_none()
        {
            source
        } else {
            destination
        };
        return Err(Stop::OutOfBounds(missing));
    }
    if src_sel == SRC_SEL_DATA {
        result.fills += 1;
    } else {
        result.copies += 1;
    }
    result.bytes_written += count as u64;
    Ok(())
}

/// `RELEASE_MEM`: event control, selectors, address, data (`ac_cmdbuf_cp.c:216-229`).
fn release_mem(
    body: &[u32],
    memory: &mut dyn CpMemory,
    result: &mut CpExecution,
) -> Result<(), Stop> {
    let [
        _event,
        selectors,
        addr_low,
        addr_high,
        data_low,
        data_high,
        ..,
    ] = *body
    else {
        return Err(Stop::Malformed);
    };
    let destination = address(addr_low, addr_high);
    let written: Vec<u8> = match selectors >> 29 {
        DATA_SEL_DISCARD => Vec::new(),
        DATA_SEL_VALUE_32BIT => data_low.to_le_bytes().to_vec(),
        DATA_SEL_VALUE_64BIT => address(data_low, data_high).to_le_bytes().to_vec(),
        DATA_SEL_TIMESTAMP => memory.timestamp().to_le_bytes().to_vec(),
        _ => return Err(Stop::NeedsGpu),
    };
    if !written.is_empty() && !memory.write(destination, &written) {
        return Err(Stop::OutOfBounds(destination));
    }
    // The last body dword is the interrupt's context id (`INT_CTXID`).
    memory.released(destination, body.get(6).copied().unwrap_or(0));
    result.releases += 1;
    result.bytes_written += written.len() as u64;
    Ok(())
}

/// `CONTEXT_CONTROL`: register state unless it enables a shadow, which writes register state to
/// memory and is refused.
fn context_control(body: &[u32]) -> Result<(), Stop> {
    let [_load, shadow, ..] = *body else {
        return Err(Stop::Malformed);
    };
    if shadow & CONTEXT_CONTROL_UPDATE_SHADOW_ENABLES != 0
        && shadow & CONTEXT_CONTROL_SHADOW_ENABLES != 0
    {
        return Err(Stop::NeedsGpu);
    }
    Ok(())
}

/// `WRITE_DATA`: control, destination low (bits 31:2) and high, then the data words
/// (`cp_pm4_table_data_gfx11.json:6677-6834`). `WR_CONFIRM` and the cache policy change nothing
/// here: the write has landed when this returns, and there is no cache before guest memory. A
/// register destination is register state, passed over as every `SET_*_REG` is; a reserved one
/// stops.
fn write_data(
    body: &[u32],
    memory: &mut dyn CpMemory,
    result: &mut CpExecution,
) -> Result<(), Stop> {
    let [control, addr_low, addr_high, ref data @ ..] = *body else {
        return Err(Stop::Malformed);
    };
    let dst_sel = (control >> 8) & 0xf;
    if dst_sel == WRITE_DATA_DST_SEL_REGISTER {
        return Ok(());
    }
    if !WRITE_DATA_DST_SEL_MEMORY.contains(&dst_sel) {
        return Err(Stop::NeedsGpu);
    }
    let destination = address(addr_low & !0x3, addr_high);
    // Without increment the words land one over another, and the last is what memory keeps.
    let words: &[u32] = if control & WRITE_DATA_NO_INCREMENT != 0 {
        data.last().map(std::slice::from_ref).unwrap_or_default()
    } else {
        data
    };
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    if !bytes.is_empty() && !memory.write(destination, &bytes) {
        return Err(Stop::OutOfBounds(destination));
    }
    result.bytes_written += bytes.len() as u64;
    Ok(())
}

/// `WAIT_REG_MEM`: function and space, address, reference, mask (`sid.h:90-94`). Only a memory wait
/// can be checked here; a register wait needs the GPU's registers.
fn wait_reg_mem(
    body: &[u32],
    memory: &mut dyn CpMemory,
    result: &mut CpExecution,
) -> Result<(), Stop> {
    let [function, addr_low, addr_high, reference, mask, ..] = *body else {
        return Err(Stop::Malformed);
    };
    if function & WAIT_MEM_SPACE == 0 {
        return Err(Stop::NeedsGpu);
    }
    let polled = address(addr_low & !0x3, addr_high);
    let word = memory.read(polled, 4).ok_or(Stop::OutOfBounds(polled))?;
    let value = u32::from_le_bytes([word[0], word[1], word[2], word[3]]) & mask;
    let holds = match function & 0x7 {
        WAIT_EQUAL => value == reference,
        WAIT_NOT_EQUAL => value != reference,
        WAIT_GREATER_OR_EQUAL => value >= reference,
        _ => return Err(Stop::NeedsGpu),
    };
    if !holds {
        return Err(Stop::Wait);
    }
    result.waits += 1;
    Ok(())
}

/// `WAIT_REG_MEM64`: function and space, address low and high, reference low and high, mask low
/// and high, poll interval (`amd_cp_packets_gfx11.h:1765-1826`). As with [`wait_reg_mem`], only a
/// memory wait with a function the 32-bit form checks is carried out.
fn wait_reg_mem64(
    body: &[u32],
    memory: &mut dyn CpMemory,
    result: &mut CpExecution,
) -> Result<(), Stop> {
    let [
        function,
        addr_low,
        addr_high,
        reference_low,
        reference_high,
        mask_low,
        mask_high,
        ..,
    ] = *body
    else {
        return Err(Stop::Malformed);
    };
    if function & WAIT_MEM_SPACE == 0 {
        return Err(Stop::NeedsGpu);
    }
    // `MEM_POLL_ADDR_LO` is bits 31:3: the low three bits are not address (`:1806`).
    let polled = address(addr_low & !0x7, addr_high);
    let bytes = memory.read(polled, 8).ok_or(Stop::OutOfBounds(polled))?;
    let mut word = [0_u8; 8];
    word.copy_from_slice(&bytes);
    let mask = u64::from(mask_high) << 32 | u64::from(mask_low);
    let value = u64::from_le_bytes(word) & mask;
    let reference = u64::from(reference_high) << 32 | u64::from(reference_low);
    let holds = match function & 0x7 {
        WAIT_EQUAL => value == reference,
        WAIT_NOT_EQUAL => value != reference,
        WAIT_GREATER_OR_EQUAL => value >= reference,
        _ => return Err(Stop::NeedsGpu),
    };
    if !holds {
        return Err(Stop::Wait);
    }
    result.waits += 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CpMemory, Stopped, WAIT_REG_MEM, execute};
    use crate::packet::build::{command_header, measured};
    use std::collections::BTreeMap;

    /// Sparse guest memory over `[0x1000, 0x9000)`; everything else is unmapped.
    #[derive(Default)]
    struct Fake {
        bytes: BTreeMap<u64, u8>,
        clock: u64,
        /// Every release reported, `(address, context)`.
        released: Vec<(u64, u32)>,
    }

    impl Fake {
        fn mapped(address: u64, length: usize) -> bool {
            address >= 0x1000 && address + length as u64 <= 0x9000
        }
        fn word(&self, address: u64) -> u32 {
            let b = |i| self.bytes.get(&(address + i)).copied().unwrap_or(0);
            u32::from_le_bytes([b(0), b(1), b(2), b(3)])
        }
    }

    impl CpMemory for Fake {
        fn read(&self, address: u64, length: usize) -> Option<Vec<u8>> {
            Self::mapped(address, length).then(|| {
                (0..length as u64)
                    .map(|i| self.bytes.get(&(address + i)).copied().unwrap_or(0))
                    .collect()
            })
        }
        fn write(&mut self, address: u64, bytes: &[u8]) -> bool {
            if !Self::mapped(address, bytes.len()) {
                return false;
            }
            for (i, b) in bytes.iter().enumerate() {
                self.bytes.insert(address + i as u64, *b);
            }
            true
        }
        fn timestamp(&mut self) -> u64 {
            self.clock += 1;
            0x1_0000_0000 + self.clock
        }
        fn released(&mut self, address: u64, context: u32) {
            self.released.push((address, context));
        }
    }

    /// A release that lands reports its address and interrupt context id, which is how a flip
    /// queued in the stream reaches the display (D728); one that cannot land reports nothing.
    #[test]
    fn a_release_reports_its_address_and_context_id() {
        let packet = |dst: u64| -> Vec<u32> {
            vec![
                command_header(measured::RELEASE_MEM, 7),
                0x0620_0504,
                0x4201_0000,
                dst as u32,
                (dst >> 32) as u32,
                1,
                0,
                0x0800_0101,
            ]
        };
        let mut memory = Fake::default();
        let ran = execute(&bytes(&packet(0x2000)), &mut memory);
        assert_eq!(ran.stopped, Stopped::Completed);
        assert_eq!(memory.released, vec![(0x2000, 0x0800_0101)]);
        assert_eq!(memory.word(0x2000), 1, "the label holds the released value");

        let mut memory = Fake::default();
        let ran = execute(&bytes(&packet(0xC_8000_40A0)), &mut memory);
        assert!(matches!(ran.stopped, Stopped::OutOfBounds { .. }));
        assert!(
            memory.released.is_empty(),
            "a release that did not land is not reported"
        );
    }

    fn fill(dst: u64, value: u32, bytes: u32) -> Vec<u32> {
        vec![
            command_header(measured::DMA_DATA, 6),
            0x8000_0000 | (2 << 29) | (3 << 20),
            value,
            0,
            dst as u32,
            (dst >> 32) as u32,
            bytes,
        ]
    }

    fn copy(src: u64, dst: u64, bytes: u32) -> Vec<u32> {
        vec![
            command_header(measured::DMA_DATA, 6),
            0x8000_0000 | (3 << 29) | (3 << 20),
            src as u32,
            (src >> 32) as u32,
            dst as u32,
            (dst >> 32) as u32,
            bytes,
        ]
    }

    fn release(dst: u64, data_sel: u32, value: u32) -> Vec<u32> {
        vec![
            command_header(measured::RELEASE_MEM, 7),
            0x0660_3514,
            data_sel << 29,
            dst as u32,
            (dst >> 32) as u32,
            value,
            0,
            0,
        ]
    }

    fn wait_equal(address: u64, reference: u32) -> Vec<u32> {
        vec![
            command_header(WAIT_REG_MEM, 6),
            0x13,
            address as u32,
            (address >> 32) as u32,
            reference,
            0xffff_ffff,
            4,
        ]
    }

    fn bytes(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    /// The wait `sceAgcDcbWaitUntilSafeForRendering` writes (`-5a17`) holds while the buffer's
    /// 64-bit label reads `0` and never holds once either half is set: a label released to `1`
    /// in its high half stops the stream there, as the command processor would wait on it forever.
    #[test]
    fn a_wait_until_safe_polls_the_whole_64_bit_label() {
        let label = 0x1000_u64;
        // The written words, then the `NOP` body the builder skips over.
        let mut wait = crate::display::wait_until_safe_words(0, label).to_vec();
        wait.resize(crate::display::WAIT_UNTIL_SAFE_DWORDS, 0);
        let mut memory = Fake::default();

        let done = execute(&bytes(&wait), &mut memory);
        assert_eq!(done.stopped, Stopped::Completed, "{done:?}");
        assert_eq!(done.waits, 1);

        let mut stream = release(label + 4, 1, 1);
        stream.extend(wait);
        let done = execute(&bytes(&stream), &mut memory);
        assert!(
            matches!(done.stopped, Stopped::WaitNeverSatisfied { .. }),
            "{done:?}"
        );
    }

    /// The GL context's clear self-test runs to completion, and the pixels are really there.
    ///
    /// Register state, a fill of the colour target, a fence, a wait on it, a copy to the readback
    /// buffer and a clock-counter fence. The fence holds `0xbeefcafe` because the fill ran, and the
    /// readback holds the fill's colour because the copy ran.
    #[test]
    fn a_clear_self_test_stream_runs_to_its_fence_and_its_readback() {
        let (target, fence, readback) = (0x2000_u64, 0x1000_u64, 0x4000_u64);
        let mut stream = vec![command_header(measured::SET_CONTEXT_REG, 2), 0x318, 0x1];
        stream.extend(fill(target, 0xff20_4060, 64));
        stream.extend(release(fence, 1, 0xbeef_cafe));
        stream.extend(wait_equal(fence, 0xbeef_cafe));
        stream.extend(copy(target, readback, 64));
        stream.extend(release(fence + 8, 3, 0));
        let mut memory = Fake::default();

        let done = execute(&bytes(&stream), &mut memory);

        assert_eq!(done.stopped, Stopped::Completed, "{done:?}");
        assert_eq!(
            (done.fills, done.copies, done.releases, done.waits),
            (1, 1, 2, 1)
        );
        assert_eq!(memory.word(fence), 0xbeef_cafe, "the fence the guest polls");
        assert_ne!(
            memory.word(fence + 8) | memory.word(fence + 12),
            0,
            "and a non-zero clock"
        );
        for pixel in 0..16 {
            assert_eq!(
                memory.word(readback + pixel * 4),
                0xff20_4060,
                "pixel {pixel}"
            );
        }
    }

    /// A draw before the fence stops execution, so no fence is written for work that did not run
    /// (D705). The fill before the draw is carried out; the `RELEASE_MEM` after it is never
    /// reached.
    #[test]
    fn a_draw_before_the_fence_leaves_the_fence_unwritten() {
        let fence = 0x1000_u64;
        let mut stream = fill(0x2000, 0x1111_1111, 16);
        stream.extend([command_header(measured::DRAW_INDEX_AUTO, 2), 3, 2]);
        stream.extend(release(fence, 1, 0xbeef_cafe));
        let mut memory = Fake::default();

        let done = execute(&bytes(&stream), &mut memory);

        assert!(
            matches!(
                done.stopped,
                Stopped::NeedsGpu {
                    opcode: measured::DRAW_INDEX_AUTO,
                    ..
                }
            ),
            "{done:?}"
        );
        assert_eq!(done.fills, 1, "the fill before the draw is real");
        assert_eq!(done.releases, 0, "the release after it is never reached");
        assert_eq!(
            memory.word(fence),
            0,
            "no fence for a draw that did not run"
        );
    }

    /// Memory whose draws the executor carries out, once, by writing a marker where they draw - or
    /// refuses, when `draws` is false.
    struct Drawing {
        inner: Fake,
        draws: bool,
        asked: usize,
    }

    impl CpMemory for Drawing {
        fn read(&self, address: u64, length: usize) -> Option<Vec<u8>> {
            self.inner.read(address, length)
        }
        fn write(&mut self, address: u64, bytes: &[u8]) -> bool {
            self.inner.write(address, bytes)
        }
        fn timestamp(&mut self) -> u64 {
            self.inner.timestamp()
        }
        fn run_draws(&mut self, _segment: &super::DrawSegment<'_>) -> bool {
            self.asked += 1;
            self.draws && self.inner.write(0x3000, &0xd4a7_d4a7_u32.to_le_bytes())
        }
    }

    fn draw() -> [u32; 3] {
        [command_header(measured::DRAW_INDEX_AUTO, 2), 3, 2]
    }

    /// Draws carried out at submit let the fence after them retire: the executor is asked once for
    /// all three draws, and what they drew is in memory before the fence is.
    #[test]
    fn draws_carried_out_together_let_the_fence_after_them_retire() {
        let fence = 0x1000_u64;
        let mut stream = fill(0x2000, 0x1111_1111, 16);
        for _ in 0..3 {
            stream.extend([command_header(measured::SET_SH_REG, 2), 0x8c, 7]);
            stream.extend(draw());
        }
        stream.extend(release(fence, 1, 0xbeef_cafe));
        let mut memory = Drawing {
            inner: Fake::default(),
            draws: true,
            asked: 0,
        };

        let done = execute(&bytes(&stream), &mut memory);

        assert_eq!(done.stopped, Stopped::Completed, "{done:?}");
        assert_eq!((done.draws, memory.asked), (3, 1), "three draws, one run");
        assert_eq!(memory.inner.word(0x3000), 0xd4a7_d4a7, "what they drew");
        assert_eq!(memory.inner.word(fence), 0xbeef_cafe, "the fence retires");
    }

    /// An executor that cannot carry the draws out leaves the fence unwritten (D705).
    #[test]
    fn draws_the_executor_refuses_still_stop_before_the_fence() {
        let fence = 0x1000_u64;
        let mut stream = draw().to_vec();
        stream.extend(release(fence, 1, 0xbeef_cafe));
        let mut memory = Drawing {
            inner: Fake::default(),
            draws: false,
            asked: 0,
        };

        let done = execute(&bytes(&stream), &mut memory);

        assert!(matches!(done.stopped, Stopped::NeedsGpu { .. }), "{done:?}");
        assert_eq!(memory.asked, 1);
        assert_eq!(memory.inner.word(fence), 0);
    }

    /// Memory whose draw segments the executor carries out in order, recording each segment it was
    /// handed and, at the moment it ran, the word at `0x2000` - what the memory work before it
    /// left there. `refuse` names a segment, by its first draw's offset, that it declines.
    struct Segmenting {
        inner: Fake,
        refuse: Option<u32>,
        seen: Vec<(u32, u32, bool, u32)>,
    }

    impl CpMemory for Segmenting {
        fn read(&self, address: u64, length: usize) -> Option<Vec<u8>> {
            self.inner.read(address, length)
        }
        fn write(&mut self, address: u64, bytes: &[u8]) -> bool {
            self.inner.write(address, bytes)
        }
        fn timestamp(&mut self) -> u64 {
            self.inner.timestamp()
        }
        fn run_draws(&mut self, segment: &super::DrawSegment<'_>) -> bool {
            self.seen.push((
                segment.first,
                segment.end,
                segment.whole,
                self.inner.word(0x2000),
            ));
            self.refuse != Some(segment.first)
        }
        /// Every dispatch carried out, writing nothing.
        fn run_dispatch(&mut self, _dispatch: &super::Dispatch<'_>) -> bool {
            true
        }
    }

    /// Memory work between draws splits them into segments carried out in order (D729): each
    /// segment's draws run when the walk reaches them, after the memory work before them and
    /// before the memory work after, and a fence between segments retires only once the draws
    /// before it ran.
    #[test]
    fn memory_work_between_draws_splits_them_into_segments_run_in_order() {
        let fence = 0x1000_u64;
        let mut stream = fill(0x2000, 0x1111_1111, 16);
        let first = u32::try_from(stream.len() * 4).expect("small");
        stream.extend(draw());
        stream.extend([command_header(measured::SET_CONTEXT_REG, 2), 0x318, 7]);
        stream.extend(draw());
        let first_end = u32::try_from(stream.len() * 4).expect("small");
        stream.extend(release(fence, 1, 1));
        stream.extend(wait_equal(fence, 1));
        stream.extend(fill(0x2000, 0x2222_2222, 16));
        let second = u32::try_from(stream.len() * 4).expect("small");
        stream.extend(draw());
        let second_end = u32::try_from(stream.len() * 4).expect("small");
        stream.extend(release(fence, 1, 2));
        let mut memory = Segmenting {
            inner: Fake::default(),
            refuse: None,
            seen: Vec::new(),
        };

        let done = execute(&bytes(&stream), &mut memory);

        assert_eq!(done.stopped, Stopped::Completed, "{done:?}");
        assert_eq!(
            memory.seen,
            vec![
                (first, first_end, false, 0x1111_1111),
                (second, second_end, false, 0x2222_2222),
            ],
            "two segments, each seeing the memory work before it and none after"
        );
        assert_eq!(done.draws, 3);
        assert_eq!(
            (done.releases, done.waits),
            (2, 1),
            "the fence between retired"
        );
        assert_eq!(memory.inner.word(fence), 2);
    }

    /// A segment the executor refuses stops the stream at its first draw: the segment before it
    /// and the fence between them retired, and nothing after it did (D705).
    #[test]
    fn a_refused_segment_stops_at_its_first_draw_after_the_segment_before_retired() {
        let fence = 0x1000_u64;
        let mut stream = draw().to_vec();
        stream.extend(release(fence, 1, 1));
        let second = u32::try_from(stream.len() * 4).expect("small");
        stream.extend(draw());
        stream.extend(release(fence, 1, 2));
        let mut memory = Segmenting {
            inner: Fake::default(),
            refuse: Some(second),
            seen: Vec::new(),
        };

        let done = execute(&bytes(&stream), &mut memory);

        assert_eq!(
            done.stopped,
            Stopped::NeedsGpu {
                offset: second,
                opcode: measured::DRAW_INDEX_AUTO
            }
        );
        assert_eq!((done.draws, done.releases), (1, 1));
        assert_eq!(memory.inner.word(fence), 1, "only the first fence");
    }

    /// Draws with nothing but register state between them are one segment holding every draw, so
    /// the executor may prepare them from the stream as it stands.
    #[test]
    fn draws_with_only_register_state_between_them_are_one_whole_segment() {
        let mut stream = draw().to_vec();
        stream.extend([command_header(measured::SET_SH_REG, 2), 0x8c, 7]);
        stream.extend(draw());
        let end = u32::try_from(stream.len() * 4).expect("small");
        stream.extend(release(0x1000, 1, 1));
        let mut memory = Segmenting {
            inner: Fake::default(),
            refuse: None,
            seen: Vec::new(),
        };

        let done = execute(&bytes(&stream), &mut memory);

        assert_eq!(done.stopped, Stopped::Completed, "{done:?}");
        assert_eq!(memory.seen, vec![(0, end, true, 0)]);
    }

    /// A stream with a dispatch in it is not drawn as prepared, even when its draws are one
    /// segment: the prepared submission carries the dispatch too, which the command processor
    /// carries out itself, so the segment is prepared from its own stream instead.
    #[test]
    fn a_lone_segment_in_a_stream_with_a_dispatch_is_not_whole() {
        let mut stream = crate::packet::build::dispatch_direct(0xc0, 1, 1).to_vec();
        let first = u32::try_from(stream.len() * 4).expect("small");
        stream.extend(draw());
        let end = u32::try_from(stream.len() * 4).expect("small");
        let mut memory = Segmenting {
            inner: Fake::default(),
            refuse: None,
            seen: Vec::new(),
        };

        let done = execute(&bytes(&stream), &mut memory);

        assert_eq!(done.stopped, Stopped::Completed, "{done:?}");
        assert_eq!(memory.seen, vec![(first, end, false, 0)]);
    }

    /// A `CONTEXT_CONTROL` that only updates its load and shadow enables, as radeonsi's preamble
    /// writes it, and a `CLEAR_STATE`, are register state: the fence after them retires.
    #[test]
    fn context_control_without_shadowing_and_clear_state_are_register_state() {
        let fence = 0x1000_u64;
        let mut stream = vec![
            command_header(super::CONTEXT_CONTROL, 2),
            0x8000_0000,
            0x8000_0000,
            command_header(super::CLEAR_STATE, 1),
            0,
        ];
        stream.extend(release(fence, 1, 0xbeef_cafe));
        let mut memory = Fake::default();

        let done = execute(&bytes(&stream), &mut memory);

        assert_eq!(done.stopped, Stopped::Completed, "{done:?}");
        assert_eq!(memory.word(fence), 0xbeef_cafe);
    }

    /// Shadow bits in a word whose `update_shadow_enables` (bit 31) is clear are not applied, so
    /// the packet changes nothing: a stream that sets them without the update bit retires.
    #[test]
    fn context_control_shadow_bits_without_their_update_bit_change_nothing() {
        let mut stream = vec![
            command_header(super::CONTEXT_CONTROL, 2),
            0x200,
            0x0101_8003,
        ];
        stream.extend(release(0x1000, 1, 0xbeef_cafe));
        let mut memory = Fake::default();

        let done = execute(&bytes(&stream), &mut memory);

        assert_eq!(done.stopped, Stopped::Completed, "{done:?}");
        assert_eq!(memory.word(0x1000), 0xbeef_cafe);
    }

    /// The three draw packets are each a draw the command processor hands to the GPU: the indexed
    /// draw from the bound buffer (`0x35`) as well as the two carrying their own.
    #[test]
    fn every_draw_packet_is_a_draw() {
        for opcode in [0x27, 0x2D, 0x35] {
            assert!(super::is_draw(opcode), "{opcode:#x}");
        }
        assert!(!super::is_draw(0x50), "DMA_DATA is not a draw");
    }

    /// A `DMA_DATA` copy whose source address is held (`SAIC`, command bit 28: `no_increment` in
    /// `cp_pm4_table_data_gfx11.json`) reads the same source dword for every destination dword:
    /// PPSA03416's `[0xe0000000, src, 0x7400, dst, 0x7400, 0x14000008]` writes one word twice.
    /// With the source through L2 (`SRC_SEL` 3) the `SAS` address space (bit 26) does not apply. A
    /// held destination (`DAIC`), or a source in register space, still stops.
    #[test]
    fn a_held_source_repeats_its_word_across_the_destination() {
        let held = vec![
            command_header(measured::DMA_DATA, 6),
            0xe000_0000,
            0x2000,
            0,
            0x3000,
            0,
            0x1400_0008,
        ];
        let mut stream = held.clone();
        stream.extend(release(0x1000, 1, 0xbeef_cafe));
        let mut memory = Fake::default();
        memory.write(0x2000, &[1, 2, 3, 4, 5, 6, 7, 8]);

        let done = execute(&bytes(&stream), &mut memory);

        assert_eq!(done.stopped, Stopped::Completed, "{done:?}");
        assert_eq!(memory.word(0x3000), 0x0403_0201);
        assert_eq!(
            memory.word(0x3004),
            0x0403_0201,
            "the same word, not the next"
        );

        let mut destination_held = held.clone();
        destination_held[6] = 0x2000_0008;
        let done = execute(&bytes(&destination_held), &mut Fake::default());
        assert_ne!(done.stopped, Stopped::Completed, "DAIC is not carried out");
        let mut register_source = held;
        register_source[1] = 0x8000_0000;
        let done = execute(&bytes(&register_source), &mut Fake::default());
        assert_ne!(
            done.stopped,
            Stopped::Completed,
            "SAS with SRC_SEL 0 is a register"
        );
    }

    /// A `DMA_DATA` with `DST_SEL` "nowhere" is radeonsi's L2 prefetch
    /// (`si_state_draw.cpp:659-664`): it reads its source and writes nothing.
    #[test]
    fn a_dma_prefetch_writes_nothing_and_retires() {
        let prefetch = vec![
            command_header(measured::DMA_DATA, 6),
            (3 << 29) | (2 << 20),
            0x2000,
            0,
            0x2000,
            0,
            0x0400_0100,
        ];
        let mut stream = prefetch.clone();
        stream.extend(release(0x1000, 1, 0xbeef_cafe));
        let mut memory = Fake::default();
        memory.write(0x2000, &[7; 0x100]);

        let done = execute(&bytes(&stream), &mut memory);

        assert_eq!(done.stopped, Stopped::Completed, "{done:?}");
        assert_eq!(memory.word(0x2000), 0x0707_0707, "nothing written over it");
        assert_eq!(done.bytes_written, 4, "only the fence");

        // A prefetch of memory that is not there faults on the hardware; here it is refused.
        let mut unmapped = prefetch;
        unmapped[2] = 0xdead_0000;
        let done = execute(&bytes(&unmapped), &mut Fake::default());
        // The stop names the packet and the address it could not reach.
        assert_eq!(
            done.stopped,
            Stopped::OutOfBounds {
                offset: 0,
                opcode: measured::DMA_DATA,
                address: 0xdead_0000,
            }
        );
    }

    /// A `CONTEXT_CONTROL` enabling any shadow writes register state to memory, which is not
    /// carried out here: it stops, and names itself.
    #[test]
    fn context_control_enabling_a_shadow_stops_the_stream() {
        for bit in [0, 1, 15, 16, 24] {
            let mut stream = vec![
                command_header(super::CONTEXT_CONTROL, 2),
                0x8000_0000,
                0x8000_0000 | (1 << bit),
            ];
            stream.extend(release(0x1000, 1, 0xbeef_cafe));
            let mut memory = Fake::default();

            let done = execute(&bytes(&stream), &mut memory);

            assert_eq!(
                done.stopped,
                Stopped::NeedsGpu {
                    offset: 0,
                    opcode: super::CONTEXT_CONTROL
                },
                "shadow bit {bit}"
            );
            assert_eq!(memory.word(0x1000), 0);
        }
    }

    fn write_data(control: u32, dst: u64, data: &[u32]) -> Vec<u32> {
        let body = u32::try_from(3 + data.len()).expect("small");
        let mut out = vec![
            command_header(super::WRITE_DATA, body),
            control,
            dst as u32,
            (dst >> 32) as u32,
        ];
        out.extend_from_slice(data);
        out
    }

    /// `WRITE_DATA` to memory - through L2, direct, or synchronised across GRBM - writes its data
    /// words at the address, in order, as radeonsi emits it (`ac_cmdbuf_cp.c:62-66`: `DST_SEL`,
    /// `WR_CONFIRM` and `ENGINE_SEL` in the control word).
    #[test]
    fn write_data_to_memory_writes_its_words() {
        for dst_sel in [1_u32, 2, 5] {
            let control = (dst_sel << 8) | (1 << 20) | (1 << 30);
            let mut stream = write_data(control, 0x2004, &[0x1111_1111, 0x2222_2222, 0x3333_3333]);
            stream.extend(release(0x1000, 1, 0xbeef_cafe));
            let mut memory = Fake::default();

            let done = execute(&bytes(&stream), &mut memory);

            assert_eq!(
                done.stopped,
                Stopped::Completed,
                "dst_sel {dst_sel}: {done:?}"
            );
            assert_eq!(
                [
                    memory.word(0x2004),
                    memory.word(0x2008),
                    memory.word(0x200c)
                ],
                [0x1111_1111, 0x2222_2222, 0x3333_3333]
            );
            assert_eq!(memory.word(0x1000), 0xbeef_cafe);
            assert_eq!(done.bytes_written, 12 + 4);
        }
    }

    /// With `ADDR_INCR` set to "do not increment" every word goes to the same address, so the last
    /// one is what memory holds.
    #[test]
    fn write_data_without_increment_leaves_the_last_word() {
        let control = (5 << 8) | (1 << 16);
        let stream = write_data(control, 0x2000, &[1, 2, 3]);
        let mut memory = Fake::default();

        let done = execute(&bytes(&stream), &mut memory);

        assert_eq!(done.stopped, Stopped::Completed, "{done:?}");
        assert_eq!((memory.word(0x2000), memory.word(0x2004)), (3, 0));
    }

    /// A register destination is register state, passed over like `SET_UCONFIG_REG`: memory is
    /// untouched and the fence after it retires. The flip builder's packet is one - `DST_SEL` 0 to
    /// register `0xc343` (obSCEne, `sceAgcDcbSetFlip`).
    #[test]
    fn write_data_to_a_register_is_register_state() {
        let mut stream = write_data(0, 0xc343, &[0x1234_5678]);
        stream.extend(release(0x1000, 1, 0xbeef_cafe));
        let mut memory = Fake::default();

        let done = execute(&bytes(&stream), &mut memory);

        assert_eq!(done.stopped, Stopped::Completed, "{done:?}");
        assert_eq!(done.bytes_written, 4, "only the fence");
        assert_eq!(memory.word(0x1000), 0xbeef_cafe);
    }

    /// A reserved destination stops, by name; an unmapped memory destination is refused.
    #[test]
    fn write_data_to_a_reserved_destination_or_unmapped_memory_stops() {
        let stream = write_data(3 << 8, 0x2000, &[1]);
        let done = execute(&bytes(&stream), &mut Fake::default());
        assert_eq!(
            done.stopped,
            Stopped::NeedsGpu {
                offset: 0,
                opcode: super::WRITE_DATA
            }
        );
        let stream = write_data(5 << 8, 0xdead_0000, &[1]);
        let done = execute(&bytes(&stream), &mut Fake::default());
        assert_eq!(
            done.stopped,
            Stopped::OutOfBounds {
                offset: 0,
                opcode: super::WRITE_DATA,
                address: 0xdead_0000,
            }
        );
    }

    /// Memory whose dispatches the executor carries out by writing where they were asked for, or
    /// refuses.
    struct Dispatching {
        inner: Fake,
        runs: bool,
        seen: Vec<(u32, [u32; 3], u32)>,
    }

    impl CpMemory for Dispatching {
        fn read(&self, address: u64, length: usize) -> Option<Vec<u8>> {
            self.inner.read(address, length)
        }
        fn write(&mut self, address: u64, bytes: &[u8]) -> bool {
            self.inner.write(address, bytes)
        }
        fn timestamp(&mut self) -> u64 {
            self.inner.timestamp()
        }
        fn run_dispatch(&mut self, dispatch: &super::Dispatch<'_>) -> bool {
            self.seen
                .push((dispatch.offset, dispatch.groups, dispatch.initiator));
            self.runs && self.inner.write(0x3000, &0xc0de_c0de_u32.to_le_bytes())
        }
    }

    /// A dispatch is handed to the executor with its place in the stream, its grid and its
    /// initiator; carried out, the fence after it retires and what it wrote is in memory before.
    /// Refused, it stops the stream and nothing after it retires.
    #[test]
    fn a_dispatch_the_executor_carries_out_lets_the_fence_after_it_retire() {
        let mut stream = fill(0x2000, 0x1111_1111, 16);
        let at = u32::try_from(stream.len() * 4).expect("small");
        stream.extend(crate::packet::build::dispatch_direct(0xc0, 1, 1));
        stream.extend(release(0x1000, 1, 0xbeef_cafe));
        for runs in [true, false] {
            let mut memory = Dispatching {
                inner: Fake::default(),
                runs,
                seen: Vec::new(),
            };

            let done = execute(&bytes(&stream), &mut memory);

            assert_eq!(memory.seen, vec![(at, [0xc0, 1, 1], 1)]);
            if runs {
                assert_eq!(done.stopped, Stopped::Completed, "{done:?}");
                assert_eq!(done.dispatches, 1);
                assert_eq!(memory.inner.word(0x3000), 0xc0de_c0de);
                assert_eq!(memory.inner.word(0x1000), 0xbeef_cafe);
            } else {
                assert_eq!(
                    done.stopped,
                    Stopped::NeedsGpu {
                        offset: at,
                        opcode: crate::packet::build::DISPATCH_DIRECT
                    }
                );
                assert_eq!(
                    memory.inner.word(0x1000),
                    0,
                    "no fence for a refused dispatch"
                );
            }
        }
    }

    /// A wait that cannot hold stops the stream; memory out of bounds is refused, not written.
    #[test]
    fn an_unsatisfiable_wait_and_an_unmapped_target_both_stop_execution() {
        let mut stream = wait_equal(0x1000, 0xbeef_cafe);
        stream.extend(release(0x1000, 1, 0x1));
        let mut memory = Fake::default();
        let done = execute(&bytes(&stream), &mut memory);
        assert!(
            matches!(done.stopped, Stopped::WaitNeverSatisfied { .. }),
            "{done:?}"
        );
        assert_eq!(memory.word(0x1000), 0, "nothing after the wait ran");

        let done = execute(&bytes(&fill(0xdead_0000, 0x5, 16)), &mut memory);
        assert!(
            matches!(done.stopped, Stopped::OutOfBounds { .. }),
            "{done:?}"
        );
        assert_eq!(done.bytes_written, 0);

        // A stream whose last packet claims more body than there is: the walk overran, so nothing
        // in it is trusted, not even the well-formed fill before the bad packet.
        let mut overrun = fill(0x2000, 0x7777_7777, 16);
        overrun.push(command_header(measured::DMA_DATA, 6));
        let done = execute(&bytes(&overrun), &mut memory);
        assert!(
            matches!(done.stopped, Stopped::Malformed { .. }),
            "{done:?}"
        );
        assert_eq!(memory.word(0x2000), 0, "an untrusted walk executes nothing");

        // A stream ending in a cut-short register write is no more trusted than any other overrun.
        // radeonsi never submits one: the only such streams seen were cut mid-packet by a chain
        // splitter scanning payload words for INDIRECT_BUFFER headers (oops-mesa 626dfd2), and
        // SuperTuxKart's builds since carry none.
        let mut zero_tail = fill(0x3000, 0x6666_6666, 16);
        zero_tail.extend([0u32; 67]);
        let done = execute(&bytes(&zero_tail), &mut memory);
        assert!(
            matches!(done.stopped, Stopped::Malformed { .. }),
            "{done:?}"
        );
        assert_eq!(memory.word(0x3000), 0, "a cut-short tail executes nothing");
    }
}
