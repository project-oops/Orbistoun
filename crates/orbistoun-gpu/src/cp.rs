//! The command processor's own memory work, carried out at submit.
//!
//! Part of a submitted stream is work the command processor does against memory with no shader
//! involved: `DMA_DATA` fills and copies, `RELEASE_MEM` end-of-pipe writes, `WAIT_REG_MEM` checks.
//! That work is reproduced exactly on the CPU. The walk goes in order and stops at the first packet
//! that needs the GPU, so a fence is written only when the work before it ran (D705). At the first
//! draw, [`CpMemory::run_draws`] may carry out all the stream's draws together, unless
//! memory work sits between them ([`Stopped::DrawsInterleaved`]). Register writes and cache control
//! are passed over: nothing here executes register state, and there is no cache before guest
//! memory. Field layouts are the public PM4 ones, cited from the collection's Mesa tree where used.

use crate::packet::{self, PacketKind, build::measured};

/// `PKT3_WAIT_REG_MEM` (`sid.h:90`). No measured builder emits it; the open-toolchain GL context
/// writes it by hand to the public layout.
pub const WAIT_REG_MEM: u8 = 0x3c;

/// `DMA_DATA` `SRC_SEL` values (`sid.h:178`, bits 30:29): the source is the packet's own data word.
const SRC_SEL_DATA: u32 = 2;
/// `DMA_DATA` `SRC_SEL`: the source is an address - direct, or through L2 (`sid.h:178`). Both are
/// the same memory here.
const SRC_SEL_ADDRESS: [u32; 2] = [0, 3];
/// `DMA_DATA` `DST_SEL` (bits 21:20): an address, direct or through L2. `1` is GDS, not memory.
const DST_SEL_ADDRESS: [u32; 2] = [0, 3];
/// `CP_DMA_ME_COMMAND.BYTE_COUNT`, bits 0-25 on this generation (`gfx103.json:12266`).
const BYTE_COUNT_MASK: u32 = 0x03ff_ffff;
/// `CP_DMA_ME_COMMAND` `SAIC`/`DAIC` (bits 28/29): hold the source/destination address rather than
/// increment it. Not reproduced; a packet asking for either stops execution.
const ADDRESS_HOLD_BITS: u32 = 0x3000_0000;

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
    /// Carries out every draw in the stream together and writes what they drew into guest memory,
    /// answering whether that happened (D712). Asked once, at the first draw, and only when no
    /// memory work sits between the first draw and the last, so running them as one equals running
    /// them in turn. The default carries out nothing, which leaves the draw as a stop.
    fn run_draws(&mut self) -> bool {
        false
    }
}

/// The draw packets a submission's draws are carried out for together: `DRAW_INDEX_2` and
/// `DRAW_INDEX_AUTO`, the two the translator turns into draws. An indirect draw reads its arguments
/// from memory at execution time and is not among them.
const fn is_draw(opcode: u8) -> bool {
    matches!(opcode, measured::DRAW_INDEX_2 | measured::DRAW_INDEX_AUTO)
}

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
    /// A packet between the stream's first draw and its last is neither a draw nor inert, so the
    /// draws cannot be carried out together without reordering them around it.
    DrawsInterleaved {
        /// Byte offset of that packet in the stream.
        offset: u32,
        /// Its opcode.
        opcode: u8,
    },
    /// A packet named memory the guest cannot read or write.
    OutOfBounds {
        /// Byte offset of the packet in the stream.
        offset: u32,
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
    /// Draw packets carried out, through [`CpMemory::run_draws`].
    pub draws: usize,
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
    // Whether the draws can run as one, decided before any of them does.
    let interleaved = interleaved_with_draws(&walked.packets);
    let mut draws_ran = false;
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
            if !draws_ran {
                if let Some((offset, opcode)) = interleaved {
                    result.stopped = Stopped::DrawsInterleaved { offset, opcode };
                    return result;
                }
                if !memory.run_draws() {
                    result.stopped = Stopped::NeedsGpu { offset, opcode };
                    return result;
                }
                draws_ran = true;
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
            _ => Err(Stop::NeedsGpu),
        };
        if let Err(stop) = step {
            result.stopped = match stop {
                Stop::NeedsGpu => Stopped::NeedsGpu { offset, opcode },
                Stop::OutOfBounds => Stopped::OutOfBounds { offset },
                Stop::Wait => Stopped::WaitNeverSatisfied { offset },
                Stop::Malformed => Stopped::Malformed { offset },
            };
            return result;
        }
    }
    result
}

/// The first packet strictly between the stream's first draw and its last that is neither a draw
/// nor memory-inert, by offset and opcode - `None` when the draws can be carried out together.
fn interleaved_with_draws(packets: &[packet::Packet]) -> Option<(u32, u8)> {
    let command = |p: &packet::Packet| match p.kind {
        PacketKind::Command { opcode } => Some(opcode),
        _ => None,
    };
    let first = packets
        .iter()
        .position(|p| command(p).is_some_and(is_draw))?;
    let last = packets
        .iter()
        .rposition(|p| command(p).is_some_and(is_draw))?;
    packets[first..last].iter().find_map(|p| {
        let opcode = command(p)?;
        let body_dwords = p.body_length() as usize / 4;
        (!is_draw(opcode) && !is_memory_inert(opcode, body_dwords)).then_some((p.offset, opcode))
    })
}

enum Stop {
    NeedsGpu,
    OutOfBounds,
    Wait,
    Malformed,
}

const fn address(low: u32, high: u32) -> u64 {
    ((high as u64) << 32) | low as u64
}

/// `DMA_DATA`: word zero, source (data or address), destination address, command
/// (`sid.h:177-184`); the byte count is 26 bits on this generation (`gfx103.json:12266`).
fn dma_data(body: &[u32], memory: &mut dyn CpMemory, result: &mut CpExecution) -> Result<(), Stop> {
    let [control, src_low, src_high, dst_low, dst_high, command, ..] = *body else {
        return Err(Stop::Malformed);
    };
    let src_sel = (control >> 29) & 0x3;
    let dst_sel = (control >> 20) & 0x3;
    if !DST_SEL_ADDRESS.contains(&dst_sel) || command & ADDRESS_HOLD_BITS != 0 {
        return Err(Stop::NeedsGpu);
    }
    let count = (command & BYTE_COUNT_MASK) as usize;
    let destination = address(dst_low, dst_high);
    // In place: a GL frame fills and copies megabytes per submission.
    let done = if count == 0 {
        true
    } else if src_sel == SRC_SEL_DATA {
        crate::perf::span(crate::perf::Span::OtherMemory, || {
            memory.fill(destination, src_low, count)
        })
    } else if SRC_SEL_ADDRESS.contains(&src_sel) {
        crate::perf::span(crate::perf::Span::Copy, || {
            memory.copy(address(src_low, src_high), destination, count)
        })
    } else {
        return Err(Stop::NeedsGpu);
    };
    if !done {
        return Err(Stop::OutOfBounds);
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
        return Err(Stop::OutOfBounds);
    }
    result.releases += 1;
    result.bytes_written += written.len() as u64;
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
    let word = memory
        .read(address(addr_low & !0x3, addr_high), 4)
        .ok_or(Stop::OutOfBounds)?;
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
        fn run_draws(&mut self) -> bool {
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

    /// Memory work between two draws refuses running them together, by name, before either runs:
    /// running both at the first would move the fill after the second draw ahead of it.
    #[test]
    fn memory_work_between_draws_is_refused_before_any_draw_runs() {
        let fence = 0x1000_u64;
        let mut stream = draw().to_vec();
        let between = u32::try_from(stream.len() * 4).expect("small");
        stream.extend(fill(0x2000, 0x2222_2222, 16));
        stream.extend(draw());
        stream.extend(release(fence, 1, 0xbeef_cafe));
        let mut memory = Drawing {
            inner: Fake::default(),
            draws: true,
            asked: 0,
        };

        let done = execute(&bytes(&stream), &mut memory);

        assert_eq!(
            done.stopped,
            Stopped::DrawsInterleaved {
                offset: between,
                opcode: measured::DMA_DATA
            }
        );
        assert_eq!(memory.asked, 0, "the executor is never asked");
        assert_eq!(memory.inner.word(fence), 0);
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
    }
}
