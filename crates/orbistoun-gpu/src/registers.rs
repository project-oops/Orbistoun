//! Register writes, and the shader addresses among them.
//!
//! A submission holds shader addresses, not shaders, written into hardware registers by
//! register-write packets. Extracting register writes is structural: a type-0 packet writes
//! consecutive registers from a base in its header, and a `SET_*_REG` packet from an index in its
//! first body word. Which register holds which half of which stage's shader address is a
//! transcription kept in `data/packets.toml`, so the results are reported as candidate addresses.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::packet::{PacketKind, PacketWalk};

/// Why a packet vocabulary could not be loaded.
///
/// Its own type: a malformed data file and a backend that cannot render something are unrelated
/// failures.
#[derive(Debug, thiserror::Error)]
pub enum VocabularyError {
    /// The file could not be parsed.
    #[error("packet vocabulary: {0}")]
    Malformed(String),
    /// An entry is individually well-formed and means nothing.
    #[error("packet vocabulary: {0}")]
    Invalid(String),
}

/// One register write observed in a submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegisterWrite {
    /// Byte offset of the packet that wrote it, so a finding can be traced back.
    pub packet_offset: u32,
    /// Register index.
    pub register: u32,
    /// Value written.
    pub value: u32,
}

/// A shader address recovered from a pair of register writes.
///
/// A candidate, because the register mapping it rests on is transcribed, not verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderCandidate {
    /// Pipeline stage the mapping attributes it to. Reported, never dispatched on.
    pub stage: String,
    /// The reassembled address.
    pub address: u64,
    /// Where the low half was written.
    pub packet_offset: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct OpcodeName {
    value: u8,
    name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RegisterWriteKind {
    opcode: u8,
    #[expect(dead_code, reason = "kept so the table reads against the reference")]
    name: String,
    base: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct ShaderAddressRegister {
    register: u32,
    stage: String,
    half: String,
}

#[derive(Debug, Deserialize, Default)]
struct VocabularyFile {
    #[serde(default)]
    opcode: Vec<OpcodeName>,
    #[serde(default)]
    register_write: Vec<RegisterWriteKind>,
    #[serde(default)]
    shader_address: Vec<ShaderAddressRegister>,
}

/// Names and mappings for the command stream.
#[derive(Debug, Clone, Default)]
pub struct Vocabulary {
    opcodes: BTreeMap<u8, String>,
    register_bases: BTreeMap<u8, u32>,
    /// register -> (stage, is_high_half)
    shader_registers: BTreeMap<u32, (String, bool)>,
}

impl Vocabulary {
    /// The registers a stage's shader address is written to - every register [`shader_candidates`]
    /// reads.
    pub fn shader_register_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.shader_registers.keys().copied()
    }

    /// Parses a vocabulary from TOML.
    pub fn load(toml_text: &str) -> Result<Self, VocabularyError> {
        let file: VocabularyFile =
            toml::from_str(toml_text).map_err(|e| VocabularyError::Malformed(e.to_string()))?;

        let mut shader_registers = BTreeMap::new();
        for entry in file.shader_address {
            // Anything but the two named halves is refused: a typo here would swap an address's
            // halves and produce plausible values wrong by four billion.
            let high = match entry.half.as_str() {
                "low" => false,
                "high" => true,
                other => {
                    return Err(VocabularyError::Invalid(format!(
                        "shader address register {:#x} has half {other:?}, expected low or high",
                        entry.register
                    )));
                }
            };
            shader_registers.insert(entry.register, (entry.stage, high));
        }

        Ok(Self {
            opcodes: file.opcode.into_iter().map(|o| (o.value, o.name)).collect(),
            register_bases: file
                .register_write
                .into_iter()
                .map(|r| (r.opcode, r.base))
                .collect(),
            shader_registers,
        })
    }

    /// The built-in vocabulary.
    pub fn builtin() -> Result<Self, VocabularyError> {
        Self::load(include_str!("../data/packets.toml"))
    }

    /// A readable name for a type-3 opcode, or `None` if the table has no entry, so a report shows
    /// the raw value and the vocabulary's gap stays visible.
    pub fn opcode_name(&self, opcode: u8) -> Option<&str> {
        self.opcodes.get(&opcode).map(String::as_str)
    }

    /// Whether this opcode writes registers, and from what base.
    pub fn register_base(&self, opcode: u8) -> Option<u32> {
        self.register_bases.get(&opcode).copied()
    }

    /// How many shader address registers are mapped.
    pub fn shader_register_count(&self) -> usize {
        self.shader_registers.len()
    }

    /// Every shader address register, as `register -> (stage, is high half)`.
    ///
    /// Exposed so a consumer can build a submission that names one, and so a report can show what
    /// the vocabulary claims.
    pub fn shader_registers(&self) -> impl Iterator<Item = (&u32, &(String, bool))> {
        self.shader_registers.iter()
    }

    /// The register-writing opcode that reaches a given register, and its base.
    ///
    /// Each register-writing opcode reaches its own class of registers from its own base, so the
    /// opcode is chosen by the register; using any register-writing opcode would underflow the
    /// offset for registers below its base. The closest base at or below the register wins, so a
    /// register in two classes' ranges resolves to the nearer one.
    pub fn opcode_for_register(&self, register: u32) -> Option<(u8, u32)> {
        self.register_bases
            .iter()
            .filter(|(_, base)| **base <= register)
            .max_by_key(|(_, base)| **base)
            .map(|(opcode, base)| (*opcode, *base))
    }
}

/// Pulls every register write out of a walked submission.
///
/// `body` is the full submitted buffer, needed because a packet's values live after
/// its header and [`PacketWalk`] records positions rather than copying them.
pub fn register_writes(
    walk: &PacketWalk,
    body: &[u8],
    vocabulary: &Vocabulary,
) -> Vec<RegisterWrite> {
    let mut writes = Vec::new();

    for packet in &walk.packets {
        let start = packet.body_offset() as usize;
        let length = packet.body_length() as usize;
        let Some(words) = read_words(body, start, length) else {
            continue;
        };

        match packet.kind {
            PacketKind::RegisterWrite { base_register } => {
                // The header names the first register; the body is the values.
                for (index, value) in words.iter().enumerate() {
                    writes.push(RegisterWrite {
                        packet_offset: packet.offset,
                        register: u32::from(base_register) + u32::try_from(index).unwrap_or(0),
                        value,
                    });
                }
            }
            PacketKind::Command { opcode } => {
                let Some(base) = vocabulary.register_base(opcode) else {
                    continue;
                };
                // Body word zero is the register offset; the rest are values. Only its low sixteen
                // bits are the offset: the indexed user-config form carries an index in bits 31:28
                // (the GL cube capture writes 0x10000242 for VGT_PRIMITIVE_TYPE).
                let Some(offset) = words.first() else {
                    continue;
                };
                let offset = offset & 0xffff;
                for (index, value) in words.iter().skip(1).enumerate() {
                    writes.push(RegisterWrite {
                        packet_offset: packet.offset,
                        register: base
                            .wrapping_add(offset)
                            .wrapping_add(u32::try_from(index).unwrap_or(0)),
                        value,
                    });
                }
            }
            PacketKind::Filler | PacketKind::Reserved => {}
        }
    }

    writes
}

/// Reassembles shader addresses from register writes.
///
/// An address needs both halves; a stage with only one half seen is skipped, since an address
/// missing its high word looks like an ordinary low address. Later writes win: a submission rebinds
/// a stage several times, and the last write before a draw is the one in force.
pub fn shader_candidates(
    writes: &[RegisterWrite],
    vocabulary: &Vocabulary,
) -> Vec<ShaderCandidate> {
    // stage -> (low, high, offset of the low write)
    let mut halves: BTreeMap<&str, (Option<u32>, Option<u32>, u32)> = BTreeMap::new();

    for write in writes {
        let Some((stage, is_high)) = vocabulary.shader_registers.get(&write.register) else {
            continue;
        };
        let entry = halves.entry(stage.as_str()).or_insert((None, None, 0));
        if *is_high {
            entry.1 = Some(write.value);
        } else {
            entry.0 = Some(write.value);
            entry.2 = write.packet_offset;
        }
    }

    halves
        .into_iter()
        .filter_map(|(stage, (low, high, offset))| {
            let (low, high) = (low?, high?);
            // The program registers hold the address in 256-byte units: the GL cube capture
            // (tests/captures/agc-gl-cube-fw1240-a) writes 0x02008f03 for the pixel shader the
            // hardware fetched from 0x2008f0300.
            Some(ShaderCandidate {
                stage: stage.to_owned(),
                address: ((u64::from(high) << 32) | u64::from(low)) << 8,
                packet_offset: offset,
            })
        })
        .collect()
}

/// The latest write to each register before a point in a stream, kept as the stream is walked
/// forward.
///
/// Everything a draw reads of the register state - shaders, blend, viewport transform, user data -
/// is the latest write to each register before the draw. This walks the writes once, in order, and
/// hands each draw the latest per register, so preparing a submission is linear in its draws.
#[derive(Debug)]
pub struct RegisterSweep<'a> {
    writes: &'a [RegisterWrite],
    next: usize,
    reached: u32,
    /// The index of each register's latest write, by register - a table rather than a map because a
    /// GL frame makes many lookups per draw. Every register a packet can name fits: the highest
    /// base, uconfig's `0xC000`, plus the widest packet's count stays below [`SWEEP_REGISTERS`];
    /// anything past it goes to `beyond`.
    table: SweepTable,
    beyond: BTreeMap<u32, usize>,
}

/// Registers the sweep's table covers directly.
const SWEEP_REGISTERS: usize = 1 << 16;

/// No write to a register yet, in a [`SweepTable`].
const UNWRITTEN: u32 = u32::MAX;

/// A sweep's register table, and the registers it has filled in.
///
/// Reused and cleared by what it touched: a fresh table is a megabyte to allocate and zero. A sweep
/// takes the spare one and returns it with only its written entries reset.
#[derive(Debug)]
struct SweepTable {
    latest: Vec<u32>,
    touched: Vec<u32>,
}

impl SweepTable {
    fn take() -> Self {
        SPARE_TABLE
            .with(std::cell::Cell::take)
            .unwrap_or_else(|| Self {
                latest: vec![UNWRITTEN; SWEEP_REGISTERS],
                touched: Vec::new(),
            })
    }

    fn clear(&mut self) {
        for register in self.touched.drain(..) {
            self.latest[register as usize] = UNWRITTEN;
        }
    }
}

thread_local! {
    /// The table the last sweep on this thread finished with, all unwritten again.
    static SPARE_TABLE: std::cell::Cell<Option<SweepTable>> = const { std::cell::Cell::new(None) };
}

impl Drop for RegisterSweep<'_> {
    fn drop(&mut self) {
        let mut table = std::mem::replace(
            &mut self.table,
            SweepTable {
                latest: Vec::new(),
                touched: Vec::new(),
            },
        );
        table.clear();
        SPARE_TABLE.with(|spare| spare.set(Some(table)));
    }
}

impl<'a> RegisterSweep<'a> {
    /// A sweep over `writes`, in the order they were made.
    #[must_use]
    pub fn new(writes: &'a [RegisterWrite]) -> Self {
        Self {
            writes,
            next: 0,
            reached: 0,
            table: SweepTable::take(),
            beyond: BTreeMap::new(),
        }
    }

    /// The latest write to each of `registers` made in packets before `before`, in the order they
    /// were made. Asked in increasing order this is one pass over the stream; asked out of order it
    /// starts again, so it is never wrong, only slower.
    pub fn before_among(
        &mut self,
        before: u32,
        registers: impl IntoIterator<Item = u32>,
    ) -> Vec<RegisterWrite> {
        self.advance(before);
        let mut indices: Vec<usize> = registers
            .into_iter()
            .filter_map(|register| self.index_of(register))
            .collect();
        indices.sort_unstable();
        indices
            .into_iter()
            .map(|index| self.writes[index])
            .collect()
    }

    /// The value of the latest write to `register` in packets before `before`, if there was one.
    ///
    /// What a per-draw lookup wants when it reads registers one at a time: the answer without
    /// gathering, sorting and rescanning a list per draw.
    pub fn latest(&mut self, before: u32, register: u32) -> Option<u32> {
        self.advance(before);
        self.index_of(register)
            .map(|index| self.writes[index].value)
    }

    fn index_of(&self, register: u32) -> Option<usize> {
        match self.table.latest.get(register as usize) {
            Some(&index) if index != UNWRITTEN => Some(index as usize),
            _ => self.beyond.get(&register).copied(),
        }
    }

    /// Takes in every write made before `before`.
    fn advance(&mut self, before: u32) {
        if before == self.reached {
            return;
        }
        if before < self.reached {
            self.next = 0;
            self.table.clear();
            self.beyond.clear();
        }
        self.reached = before;
        while let Some(write) = self.writes.get(self.next) {
            if write.packet_offset >= before {
                break;
            }
            // A stream of more writes than a table index holds keeps the rest beside it, as it does
            // registers past the table.
            let index = u32::try_from(self.next).ok().filter(|&i| i != UNWRITTEN);
            match (self.table.latest.get_mut(write.register as usize), index) {
                (Some(slot), Some(index)) => {
                    if *slot == UNWRITTEN {
                        self.table.touched.push(write.register);
                    }
                    *slot = index;
                }
                (slot, _) => {
                    // The table's older write is no longer the latest.
                    if let Some(slot) = slot {
                        *slot = UNWRITTEN;
                    }
                    self.beyond.insert(write.register, self.next);
                }
            }
            self.next += 1;
        }
    }
}

/// The shader addresses in force at a packet: [`shader_candidates`] over only the writes made in
/// earlier packets.
///
/// A stream that changes a stage's program between draws has a different shader in force at each,
/// and the stream's last write names only the one the final draws ran.
#[must_use]
pub fn shader_candidates_before(
    writes: &[RegisterWrite],
    vocabulary: &Vocabulary,
    before: u32,
) -> Vec<ShaderCandidate> {
    let earlier: Vec<RegisterWrite> = writes
        .iter()
        .filter(|write| write.packet_offset < before)
        .copied()
        .collect();
    shader_candidates(&earlier, vocabulary)
}

/// One draw a submission asked for.
///
/// The opcode names come from `data/packets.toml`, and obSCEne's measurement of the builders
/// agrees: `DcbDrawIndexAuto` is twelve bytes with header `0xc0012d00` (opcode `0x2d`, two body
/// words), and `DcbSetNumInstances` is eight with `0xc0002f00`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawCall {
    /// Byte offset of the packet that asked for it.
    pub packet_offset: u32,
    /// Instances, from the most recent instance-count packet before it.
    pub instances: u32,
    /// How the draw sources the vertices it issues.
    pub kind: DrawKind,
}

/// How a draw sources the vertices it issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawKind {
    /// Auto-generated indices (`DRAW_INDEX_AUTO`): the count is the draw's own and no index
    /// buffer is read.
    Auto {
        /// Vertices per instance.
        vertices: u32,
    },
    /// Indexed (`DRAW_INDEX_2`): `indices` values are fetched from the index buffer at `address`,
    /// both carried in the draw packet's own measured body rather than in separate state.
    Indexed {
        /// Indices per instance.
        indices: u32,
        /// Base address of the index buffer the draw reads.
        address: u64,
    },
}

/// Opcode of the auto-indexed draw.
const DRAW_INDEX_AUTO: u8 = 0x2D;

/// Opcode of the packet carrying the instance count.
const NUM_INSTANCES: u8 = 0x2F;

/// Opcode of the indexed draw, whose body carries its own index count and buffer address.
const DRAW_INDEX_2: u8 = 0x27;

/// Every draw a submission asked for, in order.
///
/// The auto-indexed draw's first body word is its vertex count, and the instance-count packet's is
/// the number of instances; the captured GL cube stream carries `c0012d00 00000003 00000002`
/// against `c0002f00 00000001`. The draw's initiator word is not read. The indexed draw is read
/// from its own measured body, `[max_size, addr_lo, addr_hi, index_count, initiator]`
/// (`packet::build::draw_index_2`): the index count is word 3 and the index-buffer address words
/// 1-2.
pub fn draw_calls(walk: &PacketWalk, body: &[u8]) -> Vec<DrawCall> {
    let mut draws = Vec::new();
    // State, in the order the stream sets it: a draw takes the instance count most recently written
    // before it, one when the stream never says.
    let mut instances = 1;

    for packet in &walk.packets {
        let PacketKind::Command { opcode } = packet.kind else {
            continue;
        };
        let start = packet.body_offset() as usize;
        let length = packet.body_length() as usize;
        let Some(words) = read_words(body, start, length) else {
            continue;
        };
        match opcode {
            NUM_INSTANCES => {
                if let Some(count) = words.first() {
                    instances = count;
                }
            }
            DRAW_INDEX_AUTO => {
                if let Some(vertices) = words.first() {
                    draws.push(DrawCall {
                        packet_offset: packet.offset,
                        instances,
                        kind: DrawKind::Auto { vertices },
                    });
                }
            }
            DRAW_INDEX_2 => {
                // Body `[max_size, addr_lo, addr_hi, index_count, initiator]`: the address is words
                // 1-2 and the count word 3. A truncated body is a desync, not a draw, and is
                // dropped rather than read past.
                if let (Some(lo), Some(hi), Some(indices)) =
                    (words.get(1), words.get(2), words.get(3))
                {
                    draws.push(DrawCall {
                        packet_offset: packet.offset,
                        instances,
                        kind: DrawKind::Indexed {
                            indices,
                            address: u64::from(lo) | (u64::from(hi) << 32),
                        },
                    });
                }
            }
            _ => {}
        }
    }
    draws
}

fn read_words(body: &[u8], start: usize, length: usize) -> Option<Words<'_>> {
    let end = start.checked_add(length)?;
    body.get(start..end).map(Words)
}

/// A packet body read as little-endian words, in place: a submission has hundreds of thousands of
/// packets, most looked at for a word or two.
#[derive(Clone, Copy)]
struct Words<'a>(&'a [u8]);

impl Words<'_> {
    fn get(self, index: usize) -> Option<u32> {
        let at = index.checked_mul(4)?;
        let bytes = self.0.get(at..at.checked_add(4)?)?;
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn first(self) -> Option<u32> {
        self.get(0)
    }

    fn iter(self) -> impl Iterator<Item = u32> {
        self.0
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
    }
}

/// A compute dispatch a submission asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchCall {
    /// Byte offset of the packet that asked for it.
    pub packet_offset: u32,
    /// Workgroup counts on X, Y and Z.
    pub groups: [u32; 3],
}

/// Opcode of the direct compute dispatch.
const DISPATCH_DIRECT: u8 = 0x15;

/// Every compute dispatch a submission asked for, in order.
///
/// `DISPATCH_DIRECT`'s body is `[x, y, z, initiator]`: the three workgroup counts, then a launch
/// flag this does not read. A body too short for the three counts is dropped, like a truncated
/// draw.
pub fn dispatch_calls(walk: &PacketWalk, body: &[u8]) -> Vec<DispatchCall> {
    let mut dispatches = Vec::new();
    for packet in &walk.packets {
        let PacketKind::Command { opcode } = packet.kind else {
            continue;
        };
        if opcode != DISPATCH_DIRECT {
            continue;
        }
        let start = packet.body_offset() as usize;
        let length = packet.body_length() as usize;
        let Some(words) = read_words(body, start, length) else {
            continue;
        };
        if let (Some(x), Some(y), Some(z)) = (words.first(), words.get(1), words.get(2)) {
            dispatches.push(DispatchCall {
                packet_offset: packet.offset,
                groups: [x, y, z],
            });
        }
    }
    dispatches
}

/// A draw or a dispatch - whichever unit of GPU work a correlation record is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawOrDispatch {
    /// A draw (`DRAW_INDEX_AUTO` or `DRAW_INDEX_2`).
    Draw(DrawCall),
    /// A compute dispatch (`DISPATCH_DIRECT`).
    Dispatch(DispatchCall),
}

impl DrawOrDispatch {
    /// Byte offset of the packet that asked for this work - what orders it within the stream.
    #[must_use]
    pub fn packet_offset(&self) -> u32 {
        match self {
            Self::Draw(draw) => draw.packet_offset,
            Self::Dispatch(dispatch) => dispatch.packet_offset,
        }
    }
}

/// A draw or dispatch, paired with the shader addresses live when it issued.
///
/// The shaders are those [`shader_candidates`] reassembles from every register write preceding the
/// work, most recent per register, so a stage rebound between two draws is attributed to each
/// correctly. The shader-address register map is transcribed, so these stay candidates. No
/// descriptor-table pointer is attached: no register mapping for one is measured (D010).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawCorrelation {
    /// The draw or dispatch, with its own decoded fields.
    pub work: DrawOrDispatch,
    /// The shader addresses live when it issued; empty when the stream bound none before it.
    pub shaders: Vec<ShaderCandidate>,
}

/// Correlates every draw and dispatch in a stream with the shader addresses live at it.
///
/// Reads the stream once into register writes, draws and dispatches, then for each unit of work,
/// ordered by position, reassembles the shader candidates from the writes preceding it. A capture
/// read from a file walks straight into this (`walk(&bytes)` then `correlate_draws`).
#[must_use]
pub fn correlate_draws(
    walk: &PacketWalk,
    body: &[u8],
    vocabulary: &Vocabulary,
) -> Vec<DrawCorrelation> {
    let writes = register_writes(walk, body, vocabulary);
    let mut work: Vec<DrawOrDispatch> = draw_calls(walk, body)
        .into_iter()
        .map(DrawOrDispatch::Draw)
        .chain(
            dispatch_calls(walk, body)
                .into_iter()
                .map(DrawOrDispatch::Dispatch),
        )
        .collect();
    work.sort_by_key(DrawOrDispatch::packet_offset);

    work.into_iter()
        .map(|unit| {
            let offset = unit.packet_offset();
            let before: Vec<RegisterWrite> = writes
                .iter()
                .copied()
                .filter(|write| write.packet_offset <= offset)
                .collect();
            DrawCorrelation {
                work: unit,
                shaders: shader_candidates(&before, vocabulary),
            }
        })
        .collect()
}

/// A buffer resource descriptor - a "V#" - decoded from its four dwords.
///
/// The concrete-value counterpart of `orbistoun_translate`'s `read_buffer_resource`, which emits
/// the same decode as arithmetic because a shader loads the descriptor at run time. Here the four
/// dwords are the ones a guest wrote, so the base address, size and stride come out as numbers. Per
/// the instruction-set reference: base address in bits 47:0, stride in 61:48, record count in
/// 95:64, swizzle-enable at bit 63 and add-thread-id at bit 119.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferDescriptor {
    /// Byte address of the buffer's base.
    pub base: u64,
    /// Bytes per record, `0..=0x3FFF`. Zero is a raw buffer whose `records` is a byte count.
    pub stride: u32,
    /// Records when there is a stride, otherwise bytes. See [`BufferDescriptor::byte_len`].
    pub records: u32,
    /// Whether the descriptor asks for addressing this project does not model - swizzled records or
    /// add-thread-id. A host cannot honour those by binding a plain range, so it is a refusal, not
    /// a buffer.
    pub unsupported: bool,
}

impl BufferDescriptor {
    /// The buffer's length in bytes: `records * stride` for a structured buffer, or `records` bytes
    /// for a raw one (stride zero).
    #[must_use]
    pub fn byte_len(&self) -> u64 {
        if self.stride == 0 {
            u64::from(self.records)
        } else {
            u64::from(self.records) * u64::from(self.stride)
        }
    }
}

/// Decodes a buffer resource descriptor from its four dwords, in register order.
///
/// The bit positions are those [`BufferDescriptor`] documents. `unsupported` folds the two
/// addressing modes this does not model (swizzle-enable, add-thread-id) so a caller refuses rather
/// than binds a range that would read the wrong memory.
#[must_use]
pub fn decode_buffer_descriptor(words: [u32; 4]) -> BufferDescriptor {
    // dword 0: base low; dword 1: base high (bits 15:0), stride (29:16), swizzle-enable (31);
    // dword 2: record count; dword 3: flags, including add-thread-id (bit 23).
    let [base_low, mid, records, flags] = words;
    BufferDescriptor {
        base: u64::from(base_low) | (u64::from(mid & 0xFFFF) << 32),
        stride: (mid >> 16) & 0x3FFF,
        records,
        unsupported: (mid & (1 << 31)) != 0 || (flags & (1 << 23)) != 0,
    }
}

/// Decodes the buffer descriptor a guest left in four consecutive scalar registers starting at
/// `first`, from the writes that set them.
///
/// [`None`] when any of the four registers was never written - a descriptor a guest never fully set
/// up is not one to bind. The most recent write to each register wins, which is the value that
/// would be live when the shader read it.
#[must_use]
pub fn buffer_descriptor_at(writes: &[RegisterWrite], first: u32) -> Option<BufferDescriptor> {
    let mut words = [0u32; 4];
    for (offset, slot) in words.iter_mut().enumerate() {
        let register = first + u32::try_from(offset).ok()?;
        *slot = writes
            .iter()
            .rev()
            .find(|w| w.register == register)
            .map(|w| w.value)?;
    }
    Some(decode_buffer_descriptor(words))
}

/// A colour render target's pixel dimensions, decoded from `CB_COLOR0_ATTRIB2`.
///
/// The width and height a draw rasterises into, which a host needs to size the attachment a
/// translated pipeline draws to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColourTargetExtent {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// The register that carries a colour target's dimensions.
///
/// `CB_COLOR0_ATTRIB2`, a context register: the `SET_CONTEXT_REG` base `0xA000` plus offset
/// `0x3B0`. The GL cube capture (tests/captures/agc-gl-cube-fw1240-a) writes `0x01dfc437` here,
/// which [`decode_colour_target_extent`] turns into `1920 x 1080`; no other pair comes out of that
/// value.
const CB_COLOR0_ATTRIB2: u32 = 0xA3B0;

/// Decodes a colour target's dimensions from a `CB_COLOR0_ATTRIB2` value.
///
/// Width minus one sits in bits 27:14 and height minus one in bits 13:0, each fourteen bits. The
/// stored value is one below the pixel count, so a target is at least one pixel on a side and the
/// GL cube's `0x01dfc437` decodes to `1920 x 1080`.
#[must_use]
pub fn decode_colour_target_extent(attrib2: u32) -> ColourTargetExtent {
    ColourTargetExtent {
        width: ((attrib2 >> 14) & 0x3FFF) + 1,
        height: (attrib2 & 0x3FFF) + 1,
    }
}

/// The colour target's dimensions, from the live value of `CB_COLOR0_ATTRIB2` among the writes:
/// the level `CB_COLOR0_VIEW.MIP_LEVEL` names of a mip chain, whose sides halve per level from
/// `ATTRIB2`'s level 0, never below one.
///
/// [`None`] when the stream never sets it: a target size is not guessed (D010). The most recent
/// write wins, the value live at the draw.
#[must_use]
pub fn colour_target_extent_at(writes: &[RegisterWrite]) -> Option<ColourTargetExtent> {
    let last = |register: u32| {
        writes
            .iter()
            .rev()
            .find(|write| write.register == register)
            .map(|write| write.value)
    };
    let attrib2 = last(CB_COLOR0_ATTRIB2)?;
    let mip0 = decode_colour_target_extent(attrib2);
    // `ATTRIB2.MAX_MIP`, 31:28: a single level is level 0 whatever the view says.
    let level = if attrib2 >> 28 == 0 {
        0
    } else {
        (last(CB_COLOR0_VIEW).unwrap_or(0) >> VIEW_MIP_LEVEL_SHIFT) & 0xF
    };
    Some(ColourTargetExtent {
        width: (mip0.width >> level).max(1),
        height: (mip0.height >> level).max(1),
    })
}

/// Level 0's dimensions, `CB_COLOR0_ATTRIB2` as written.
fn colour_target_mip0_extent_at(writes: &[RegisterWrite]) -> Option<ColourTargetExtent> {
    let value = writes
        .iter()
        .rev()
        .find(|write| write.register == CB_COLOR0_ATTRIB2)?
        .value;
    Some(decode_colour_target_extent(value))
}

/// The register that carries colour buffer zero's base address.
///
/// `CB_COLOR0_BASE`, a context register at index `0xA318`: `src/amd/registers/gfx103.json` in
/// oops-mesa maps it at byte `167008` = `0x28C60`, context dword `(0x28C60 - 0x28000) / 4` =
/// `0x318`. The value is the address in 256-byte units, like [`ImageDescriptor`]'s base, so the
/// byte address is `value << 8`.
const CB_COLOR0_BASE: u32 = 0xA318;

/// Colour buffer zero, as a submission set it up: where it is and how big.
///
/// The address, width and height a host needs to make the render target resident; the base is what
/// [`crate::tiling`] slices before detiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColourTarget {
    /// Base byte address of the surface in guest memory - for a `64KB_R_X` surface, its first
    /// block.
    pub base: u64,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The surface's pipe-bank XOR, which moves 256-byte runs within each block
    /// ([`crate::tiling`]); zero for a surface without one.
    pub pipe_bank_xor: u8,
    /// How its texels lie in memory: the layout its swizzle mode names, `64KB_R_X` where the
    /// mode is not one modelled (a target is written back only in a modelled one).
    pub layout: crate::tiling::SurfaceLayout,
    /// Where its texels lie in the surface its words span.
    pub place: Place,
}

/// Where a level's texels lie in the surface its words span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Place {
    /// The surface is exactly the level: its words are a surface of the level's own extent.
    #[default]
    Whole,
    /// The level lies at `origin` in a surface of `span` texels: a mip-tail level in the tail's
    /// block, shared with the tail's other levels, or a level whose chain stores it padded - an
    /// odd side's level is laid out rounded up (`GetMipSize`) while its texels are rounded down.
    Within {
        /// The surface's extent.
        span: (u32, u32),
        /// Where the level's `(0, 0)` is in it.
        origin: (u32, u32),
    },
}

/// Level `level` of a `levels`-level 2D chain at `base`, level 0 `width` x `height`, laid out as
/// `layout` lays a chain out: where it starts, its extent - each side halved per level rounding
/// down, never below one - and where its texels lie in what its words span. `None` for a level
/// past the chain, or one whose place `layout` does not give.
#[must_use]
pub fn chain_level(
    (base, pipe_bank_xor): (u64, u8),
    (width, height): (u32, u32),
    (levels, level): (u32, u32),
    layout: crate::tiling::SurfaceLayout,
) -> Option<ColourTarget> {
    let level = if levels <= 1 { 0 } else { level };
    if level >= levels.max(1) {
        return None;
    }
    let extent = ((width >> level).max(1), (height >> level).max(1));
    let (offset, place) = if levels <= 1 {
        (0, Place::Whole)
    } else if let Some(offset) = layout.level_offset(width, height, levels, level) {
        let span = crate::tiling::mip_layout_extent(width, height, level);
        let place = if span == extent {
            Place::Whole
        } else {
            Place::Within {
                span,
                origin: (0, 0),
            }
        };
        (offset, place)
    } else {
        let origin = layout.tail_origin(width, height, levels, level)?;
        (
            0,
            Place::Within {
                span: layout.block_texels(),
                origin,
            },
        )
    };
    Some(ColourTarget {
        base: base + offset,
        width: extent.0,
        height: extent.1,
        pipe_bank_xor,
        layout,
        place,
    })
}

impl ColourTarget {
    /// The extent of the surface its words span: its own, or the one it lies within.
    const fn span_extent(&self) -> (u32, u32) {
        match self.place {
            Place::Within { span, .. } => span,
            Place::Whole => (self.width, self.height),
        }
    }

    /// Words its memory spans from [`Self::base`]: the whole tail block for a level in one.
    #[must_use]
    pub fn words(&self) -> usize {
        let (width, height) = self.span_extent();
        self.layout.words(width, height)
    }

    /// Detiles it into a linear, row-major image of its own extent, each texel through `map`.
    ///
    /// # Panics
    ///
    /// When `tiled` is shorter than [`Self::words`].
    #[must_use]
    pub fn detile_mapped(&self, tiled: &[u32], map: impl Fn(u32) -> u32 + Sync) -> Vec<u32> {
        let (span_width, span_height) = self.span_extent();
        let whole =
            self.layout
                .detile_mapped(tiled, span_width, span_height, self.pipe_bank_xor, map);
        match self.place {
            Place::Whole => whole,
            Place::Within { origin: (x, y), .. } => (y..y + self.height)
                .flat_map(|row| {
                    let start = (row * span_width + x) as usize;
                    whole[start..start + self.width as usize].iter().copied()
                })
                .collect(),
        }
    }

    /// Tiles a linear, row-major image of its own extent into it, each texel through `map`,
    /// leaving every other word - the tail's other levels, or a padded level's padding, among
    /// them - as it was.
    ///
    /// # Errors
    ///
    /// [`crate::tiling::DetileError::TiledDataTooShort`] when `tiled` does not cover
    /// [`Self::words`] or `linear` is not its extent.
    pub fn tile_mapped(
        &self,
        linear: &[u32],
        tiled: &mut [u32],
        map: impl Fn(u32) -> u32 + Sync,
    ) -> Result<(), crate::tiling::DetileError> {
        let Place::Within { origin: (x, y), .. } = self.place else {
            return self.layout.tile_mapped(
                linear,
                self.width,
                self.height,
                self.pipe_bank_xor,
                tiled,
                map,
            );
        };
        let (span_width, span_height) = self.span_extent();
        let texels = self.width as usize * self.height as usize;
        if tiled.len() < self.words() || linear.len() != texels {
            return Err(crate::tiling::DetileError::TiledDataTooShort {
                needed_words: self.words().max(texels),
                got_words: tiled.len().min(linear.len()),
            });
        }
        let mut whole =
            self.layout
                .detile_mapped(tiled, span_width, span_height, self.pipe_bank_xor, |w| w);
        for (row, texels) in linear.chunks(self.width as usize).enumerate() {
            let start = ((y + row as u32) * span_width + x) as usize;
            for (slot, &texel) in whole[start..start + texels.len()].iter_mut().zip(texels) {
                *slot = map(texel);
            }
        }
        self.layout.tile_mapped(
            &whole,
            span_width,
            span_height,
            self.pipe_bank_xor,
            tiled,
            |w| w,
        )
    }
}

/// The colour target a submission set up, from the live values of `CB_COLOR0_BASE` and
/// `CB_COLOR0_ATTRIB2` among the writes.
///
/// [`None`] unless both were set: a missing half is not invented (D010). The most recent write to
/// each wins. A `64KB_R_X` surface starts on a 64 KiB block, so the low byte of its 256-byte base
/// is not address: radeonsi ORs the surface's pipe-bank XOR in there (`ac_descriptors.c:1477-1481`,
/// `cb_color_base |= tile_swizzle`), and it is split out.
///
/// The base is the whole surface's, and `CB_COLOR0_VIEW.MIP_LEVEL` picks the level drawn: the
/// target is that level, where the chain places it ([`crate::tiling::SurfaceLayout::level_offset`])
/// at its own extent, or a level in the mip tail at the tail's block with its origin there. [`None`]
/// for a chain in a swizzle mode whose chains are not modelled, or a level past the chain.
#[must_use]
pub fn colour_target_at(writes: &[RegisterWrite]) -> Option<ColourTarget> {
    let last = |register: u32| {
        writes
            .iter()
            .rev()
            .find(|write| write.register == register)
            .map(|write| write.value)
    };
    let base = last(CB_COLOR0_BASE)?;
    let extent = colour_target_mip0_extent_at(writes)?;
    let mode = colour_swizzle_mode_at(writes);
    // The pipe-bank XOR rides in the base's low byte for `64KB_R_X`. A `4KB_D_X` base is only
    // 4 KiB aligned, so its low byte is address, and the base is taken whole.
    let (base, pipe_bank_xor) = match mode {
        Some(SwizzleMode::Tiled64KbRX) => (base & !0xFF, (base & 0xFF) as u8),
        _ => (base, 0),
    };
    let layout = mode.and_then(crate::tiling::SurfaceLayout::of);
    // `ATTRIB2.MAX_MIP`, 31:28; an unwritten `VIEW` reads as its reset value, level 0.
    let levels = (last(CB_COLOR0_ATTRIB2)? >> 28) + 1;
    let level = (last(CB_COLOR0_VIEW).unwrap_or(0) >> VIEW_MIP_LEVEL_SHIFT) & 0xF;
    // A chain's level lies where the layout places it; a single level in an unmodelled mode keeps
    // its base, and is written back only if its mode is modelled.
    let layout = match layout {
        Some(layout) => layout,
        None if levels == 1 => crate::tiling::SurfaceLayout::default(),
        None => return None,
    };
    chain_level(
        (u64::from(base) << 8, pipe_bank_xor),
        (extent.width, extent.height),
        (levels, level),
        layout,
    )
}

/// `CB_COLOR0_VIEW` (`gfx103.json`, byte `167020`, dword `0xA31B`): `SLICE_START` 12:0,
/// `SLICE_MAX` 25:13, `MIP_LEVEL` 29:26.
const CB_COLOR0_VIEW: u32 = 0xA31B;
/// `CB_COLOR0_VIEW`'s `SLICE_START` and `SLICE_MAX` together.
const VIEW_SLICES: u32 = 0x03FF_FFFF;
/// `CB_COLOR0_VIEW.MIP_LEVEL`'s shift; the field is four bits.
const VIEW_MIP_LEVEL_SHIFT: u32 = 26;
/// `CB_COLOR0_ATTRIB` (`gfx103.json`, byte `167028`, dword `0xA31D`): `NUM_SAMPLES` 14:12,
/// `NUM_FRAGMENTS` 16:15.
const CB_COLOR0_ATTRIB: u32 = 0xA31D;
/// `CB_COLOR0_DCC_BASE` (`gfx103.json`, byte `167060`, dword `0xA325`), in 256-byte units.
const CB_COLOR0_DCC_BASE: u32 = 0xA325;
/// `CB_COLOR0_DCC_BASE_EXT` (`gfx103.json`, byte `167584`, dword `0xA3A8`): address bits 47:40 in
/// `BASE_256B` 7:0.
const CB_COLOR0_DCC_BASE_EXT: u32 = 0xA3A8;
/// `CB_COLOR0_INFO.DCC_ENABLE`, bit 28 (`gfx103.json`).
const INFO_DCC_ENABLE: u32 = 1 << 28;
/// `CB_COLOR0_ATTRIB3.DCC_PIPE_ALIGNED`, bit 30 (`gfx103.json`).
const ATTRIB3_DCC_PIPE_ALIGNED: u32 = 1 << 30;
/// `CB_COLOR0_ATTRIB3.RESOURCE_TYPE`'s value for a 2D surface, `ADDR_RSRC_TEX_2D`
/// (`addrtypes.h:310`), which radeonsi writes there (`ac_descriptors.c:1328-1329`).
const RESOURCE_TYPE_2D: u32 = 1;

/// Colour target zero's delta colour compression, when `CB_COLOR0_INFO.DCC_ENABLE` is set: where
/// its keys are, the whole surface they describe, and whether that surface is the single-slice,
/// single-sample 2D one whose keys are one run of whole metadata blocks, every level's together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColourTargetDcc {
    /// The keys' base and layout.
    pub dcc: crate::dcc::Dcc,
    /// The surface's first byte, `CB_COLOR0_BASE` without its pipe-bank XOR: level 0 of a single
    /// level, the mip tail's block of a chain.
    pub surface: u64,
    /// Level 0's extent (`ATTRIB2`).
    pub extent: ColourTargetExtent,
    /// Levels in the chain, `ATTRIB2.MAX_MIP` plus one.
    pub levels: u32,
    /// One 2D slice: `CB_COLOR0_VIEW`'s slices and `ATTRIB3.MIP0_DEPTH` all zero,
    /// `ATTRIB3.RESOURCE_TYPE` 2D.
    pub single_slice: bool,
    /// `CB_COLOR0_ATTRIB`'s sample and fragment counts both one.
    pub single_sample: bool,
}

/// Colour target zero's DCC from the live registers, or `None` when `CB_COLOR0_INFO` is unset or
/// does not enable it. An unwritten `VIEW`, `ATTRIB` or `DCC_BASE_EXT` reads as its reset value,
/// zero, which `CLEAR_STATE` leaves them at.
#[must_use]
pub fn colour_target_dcc_at(writes: &[RegisterWrite]) -> Option<ColourTargetDcc> {
    let last = |register: u32| {
        writes
            .iter()
            .rev()
            .find(|write| write.register == register)
            .map(|write| write.value)
    };
    if last(CB_COLOR0_INFO)? & INFO_DCC_ENABLE == 0 {
        return None;
    }
    let base = (u64::from(last(CB_COLOR0_DCC_BASE_EXT).unwrap_or(0) & 0xFF) << 40)
        | (u64::from(last(CB_COLOR0_DCC_BASE)?) << 8);
    let attrib3 = last(CB_COLOR0_ATTRIB3)?;
    let attrib2 = last(CB_COLOR0_ATTRIB2)?;
    // `ATTRIB3.MIP0_DEPTH` 12:0, `RESOURCE_TYPE` 25:24.
    let mip0_depth = attrib3 & 0x1FFF;
    let single_slice = last(CB_COLOR0_VIEW).unwrap_or(0) & VIEW_SLICES == 0
        && mip0_depth == 0
        && (attrib3 >> 24) & 0x3 == RESOURCE_TYPE_2D;
    // `NUM_SAMPLES` and `NUM_FRAGMENTS`, as `log2`, together in 16:12.
    let samples_and_fragments = (last(CB_COLOR0_ATTRIB).unwrap_or(0) >> 12) & 0x1F;
    Some(ColourTargetDcc {
        dcc: crate::dcc::Dcc {
            base,
            pipe_aligned: attrib3 & ATTRIB3_DCC_PIPE_ALIGNED != 0,
        },
        surface: u64::from(last(CB_COLOR0_BASE)? & !0xFF) << 8,
        extent: decode_colour_target_extent(attrib2),
        levels: (attrib2 >> 28) + 1,
        single_slice,
        single_sample: samples_and_fragments == 0,
    })
}

/// The register that carries which colour targets are written, and on which channels.
///
/// `CB_TARGET_MASK`, a context register at index `0xA08E`: `src/amd/registers/gfx103.json` in
/// oops-mesa maps it at byte `164408` = `0x28238`, context dword `0x8E`, with eight four-bit fields
/// `TARGET0_ENABLE`..`TARGET7_ENABLE` at bits `[0,3]`..`[28,31]`.
const CB_TARGET_MASK: u32 = 0xA08E;

/// Which of the eight colour targets a submission writes, and on which channels.
///
/// One four-bit channel mask per MRT (bit 0 red, 1 green, 2 blue, 3 alpha), so `0xF` writes all
/// four and `0` disables the target. A host needs it to know how many attachments to configure and
/// which components a draw writes - a target left out of the mask is not rendered to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetMask {
    /// Per-target channel write mask, indexed by MRT `0..8`; each is four bits.
    pub targets: [u32; 8],
}

impl TargetMask {
    /// Whether colour target `index` writes any channel.
    #[must_use]
    pub fn writes_target(&self, index: usize) -> bool {
        self.targets.get(index).is_some_and(|&mask| mask != 0)
    }

    /// How many colour targets write at least one channel.
    #[must_use]
    pub fn active_targets(&self) -> usize {
        self.targets.iter().filter(|&&mask| mask != 0).count()
    }
}

/// Decodes `CB_TARGET_MASK` into its eight per-target channel masks.
#[must_use]
pub fn decode_target_mask(value: u32) -> TargetMask {
    let mut targets = [0u32; 8];
    for (index, mask) in targets.iter_mut().enumerate() {
        *mask = (value >> (index * 4)) & 0xF;
    }
    TargetMask { targets }
}

/// The colour write mask a submission set, from the live value of `CB_TARGET_MASK` among the
/// writes.
///
/// [`None`] when the stream never sets it, rather than a default of "all" or "none".
/// Most-recent-write-wins.
#[must_use]
pub fn target_mask_at(writes: &[RegisterWrite]) -> Option<TargetMask> {
    let value = writes
        .iter()
        .rev()
        .find(|write| write.register == CB_TARGET_MASK)?
        .value;
    Some(decode_target_mask(value))
}

/// A depth or stencil comparison - the `CompareFrag` enum every test field in `DB_DEPTH_CONTROL`
/// selects from.
///
/// Cited from oops-mesa `src/amd/registers/gfx103.json` (`CompareFrag`: `FRAG_NEVER` 0, `FRAG_LESS`
/// 1, `FRAG_EQUAL` 2, `FRAG_LEQUAL` 3, `FRAG_GREATER` 4, `FRAG_NOTEQUAL` 5, `FRAG_GEQUAL` 6,
/// `FRAG_ALWAYS` 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompareFunc {
    /// The test never passes.
    Never,
    /// Passes when the fragment value is less than the stored one.
    Less,
    /// Passes on equality.
    Equal,
    /// Passes when less than or equal.
    LessEqual,
    /// Passes when greater than.
    Greater,
    /// Passes on inequality.
    NotEqual,
    /// Passes when greater than or equal.
    GreaterEqual,
    /// The test always passes.
    Always,
}

/// Decodes a three-bit `CompareFrag` field into its comparison.
///
/// The field is three bits, so masking to `0..8` makes the match total - every value is one of the
/// eight comparisons, none reserved.
#[must_use]
pub fn decode_compare_func(field: u32) -> CompareFunc {
    match field & 0x7 {
        0 => CompareFunc::Never,
        1 => CompareFunc::Less,
        2 => CompareFunc::Equal,
        3 => CompareFunc::LessEqual,
        4 => CompareFunc::Greater,
        5 => CompareFunc::NotEqual,
        6 => CompareFunc::GreaterEqual,
        _ => CompareFunc::Always,
    }
}

/// `DB_DEPTH_CONTROL`, a context register at index `0xA200`: `src/amd/registers/gfx103.json` in
/// oops-mesa maps it at byte `165888` and defines its fields.
pub(crate) const DB_DEPTH_CONTROL: u32 = 0xA200;

/// The depth- and stencil-test state a draw runs under, decoded from `DB_DEPTH_CONTROL`.
///
/// Whether each test is on, whether depth is written, and the comparison each uses. The two
/// colour-write-on-depth-{fail,pass} bits (30, 31) are not decoded.
// Each bool is one of `DB_DEPTH_CONTROL`'s independent enable bits, named so a reader sees which
// test a value turned on; the "too many bools" lint does not fit a register mirror.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DepthControl {
    /// `Z_ENABLE` (bit 1): the depth test runs.
    pub depth_test_enable: bool,
    /// `Z_WRITE_ENABLE` (bit 2): a passing fragment updates the depth buffer.
    pub depth_write_enable: bool,
    /// `ZFUNC` (bits 4:6): how a fragment's depth is compared with the stored value.
    pub depth_func: CompareFunc,
    /// `DEPTH_BOUNDS_ENABLE` (bit 3): the depth-bounds test runs.
    pub depth_bounds_test_enable: bool,
    /// `STENCIL_ENABLE` (bit 0): the stencil test runs.
    pub stencil_test_enable: bool,
    /// `STENCILFUNC` (bits 8:10): the front-face stencil comparison.
    pub stencil_func: CompareFunc,
    /// `BACKFACE_ENABLE` (bit 7): back faces use their own stencil state rather than the front's.
    pub backface_enable: bool,
    /// `STENCILFUNC_BF` (bits 20:22): the back-face stencil comparison, used when
    /// `backface_enable`.
    pub stencil_func_backface: CompareFunc,
}

/// Decodes `DB_DEPTH_CONTROL` into its depth- and stencil-test state.
#[must_use]
pub fn decode_depth_control(value: u32) -> DepthControl {
    DepthControl {
        depth_test_enable: (value >> 1) & 1 != 0,
        depth_write_enable: (value >> 2) & 1 != 0,
        depth_func: decode_compare_func(value >> 4),
        depth_bounds_test_enable: (value >> 3) & 1 != 0,
        stencil_test_enable: value & 1 != 0,
        stencil_func: decode_compare_func(value >> 8),
        backface_enable: (value >> 7) & 1 != 0,
        stencil_func_backface: decode_compare_func(value >> 20),
    }
}

/// The depth- and stencil-test state a submission set, from the live value of `DB_DEPTH_CONTROL`.
///
/// [`None`] when the stream never sets it, rather than a default. Most-recent-write-wins.
#[must_use]
pub fn depth_control_at(writes: &[RegisterWrite]) -> Option<DepthControl> {
    let value = writes
        .iter()
        .rev()
        .find(|write| write.register == DB_DEPTH_CONTROL)?
        .value;
    Some(decode_depth_control(value))
}

/// A stencil operation - the `StencilOp` enum each of `DB_STENCIL_CONTROL`'s six fields selects.
///
/// What happens to a stencil value on each test outcome. Cited from oops-mesa
/// `src/amd/registers/gfx103.json` (`StencilOp`: `STENCIL_KEEP` 0 .. `STENCIL_XNOR` 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StencilOp {
    /// Keep the current value.
    Keep,
    /// Set to zero.
    Zero,
    /// Set to all-ones.
    Ones,
    /// Replace with the reference, and test.
    ReplaceTest,
    /// Replace with the reference.
    ReplaceOp,
    /// Increment, clamping at the maximum.
    AddClamp,
    /// Decrement, clamping at zero.
    SubClamp,
    /// Bitwise invert.
    Invert,
    /// Increment, wrapping.
    AddWrap,
    /// Decrement, wrapping.
    SubWrap,
    /// Bitwise AND with the reference.
    And,
    /// Bitwise OR.
    Or,
    /// Bitwise XOR.
    Xor,
    /// Bitwise NAND.
    Nand,
    /// Bitwise NOR.
    Nor,
    /// Bitwise XNOR.
    Xnor,
}

/// Decodes a four-bit `StencilOp` field.
///
/// The field is four bits and every value `0..16` is defined, so masking makes the match total.
#[must_use]
pub fn decode_stencil_op(field: u32) -> StencilOp {
    match field & 0xF {
        0 => StencilOp::Keep,
        1 => StencilOp::Zero,
        2 => StencilOp::Ones,
        3 => StencilOp::ReplaceTest,
        4 => StencilOp::ReplaceOp,
        5 => StencilOp::AddClamp,
        6 => StencilOp::SubClamp,
        7 => StencilOp::Invert,
        8 => StencilOp::AddWrap,
        9 => StencilOp::SubWrap,
        10 => StencilOp::And,
        11 => StencilOp::Or,
        12 => StencilOp::Xor,
        13 => StencilOp::Nand,
        14 => StencilOp::Nor,
        _ => StencilOp::Xnor,
    }
}

/// `DB_STENCIL_CONTROL`, a context register at index `0xA10B`: `src/amd/registers/gfx103.json` in
/// oops-mesa maps it at byte `164908` and defines its six four-bit `StencilOp` fields.
pub(crate) const DB_STENCIL_CONTROL: u32 = 0xA10B;

/// The stencil operations a draw applies on each test outcome, decoded from `DB_STENCIL_CONTROL`.
///
/// Front and back faces each carry three operations: what to do when the stencil test fails, when
/// it passes and the depth test passes, and when it passes but the depth test fails. The back-face
/// set applies only when `DB_DEPTH_CONTROL`'s back-face stencil is on
/// ([`DepthControl::backface_enable`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StencilControl {
    /// `STENCILFAIL` (bits 0:3): front-face op when the stencil test fails.
    pub fail_op: StencilOp,
    /// `STENCILZPASS` (bits 4:7): front-face op when stencil and depth both pass.
    pub depth_pass_op: StencilOp,
    /// `STENCILZFAIL` (bits 8:11): front-face op when stencil passes but depth fails.
    pub depth_fail_op: StencilOp,
    /// `STENCILFAIL_BF` (bits 12:15): back-face op when the stencil test fails.
    pub back_fail_op: StencilOp,
    /// `STENCILZPASS_BF` (bits 16:19): back-face op when stencil and depth both pass.
    pub back_depth_pass_op: StencilOp,
    /// `STENCILZFAIL_BF` (bits 20:23): back-face op when stencil passes but depth fails.
    pub back_depth_fail_op: StencilOp,
}

/// Decodes `DB_STENCIL_CONTROL` into its six stencil operations.
#[must_use]
pub fn decode_stencil_control(value: u32) -> StencilControl {
    StencilControl {
        fail_op: decode_stencil_op(value),
        depth_pass_op: decode_stencil_op(value >> 4),
        depth_fail_op: decode_stencil_op(value >> 8),
        back_fail_op: decode_stencil_op(value >> 12),
        back_depth_pass_op: decode_stencil_op(value >> 16),
        back_depth_fail_op: decode_stencil_op(value >> 20),
    }
}

/// The stencil operations a submission set, from the live value of `DB_STENCIL_CONTROL`.
///
/// [`None`] when the stream never sets it. Most-recent-write-wins.
#[must_use]
pub fn stencil_control_at(writes: &[RegisterWrite]) -> Option<StencilControl> {
    let value = writes
        .iter()
        .rev()
        .find(|write| write.register == DB_STENCIL_CONTROL)?
        .value;
    Some(decode_stencil_control(value))
}

/// A blend factor - the `BlendOp` enum each source/destination field of `CB_BLEND0_CONTROL`
/// selects.
///
/// What a colour or alpha channel is multiplied by before the combine function. Cited from
/// oops-mesa `src/amd/registers/gfx103.json` (`BlendOp`: `BLEND_ZERO` 0 ..
/// `BLEND_ONE_MINUS_CONSTANT_ALPHA` 20). The field is five bits and `21..32` are reserved, carried
/// as [`BlendFactor::Other`] rather than mapped to a defined factor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlendFactor {
    /// Multiply by zero.
    Zero,
    /// Multiply by one.
    One,
    /// The source colour.
    SrcColor,
    /// One minus the source colour.
    OneMinusSrcColor,
    /// The source alpha.
    SrcAlpha,
    /// One minus the source alpha.
    OneMinusSrcAlpha,
    /// The destination alpha.
    DstAlpha,
    /// One minus the destination alpha.
    OneMinusDstAlpha,
    /// The destination colour.
    DstColor,
    /// One minus the destination colour.
    OneMinusDstColor,
    /// The source alpha, saturated.
    SrcAlphaSaturate,
    /// Both source alpha.
    BothSrcAlpha,
    /// Both inverse source alpha.
    BothInvSrcAlpha,
    /// The blend constant colour.
    ConstantColor,
    /// One minus the blend constant colour.
    OneMinusConstantColor,
    /// The second dual-source colour.
    Src1Color,
    /// One minus the second dual-source colour.
    InvSrc1Color,
    /// The second dual-source alpha.
    Src1Alpha,
    /// One minus the second dual-source alpha.
    InvSrc1Alpha,
    /// The blend constant alpha.
    ConstantAlpha,
    /// One minus the blend constant alpha.
    OneMinusConstantAlpha,
    /// A reserved encoding (`21..32`), kept raw rather than guessed.
    Other(u32),
}

/// Decodes a five-bit `BlendOp` field into its blend factor.
#[must_use]
pub fn decode_blend_factor(field: u32) -> BlendFactor {
    match field & 0x1F {
        0 => BlendFactor::Zero,
        1 => BlendFactor::One,
        2 => BlendFactor::SrcColor,
        3 => BlendFactor::OneMinusSrcColor,
        4 => BlendFactor::SrcAlpha,
        5 => BlendFactor::OneMinusSrcAlpha,
        6 => BlendFactor::DstAlpha,
        7 => BlendFactor::OneMinusDstAlpha,
        8 => BlendFactor::DstColor,
        9 => BlendFactor::OneMinusDstColor,
        10 => BlendFactor::SrcAlphaSaturate,
        11 => BlendFactor::BothSrcAlpha,
        12 => BlendFactor::BothInvSrcAlpha,
        13 => BlendFactor::ConstantColor,
        14 => BlendFactor::OneMinusConstantColor,
        15 => BlendFactor::Src1Color,
        16 => BlendFactor::InvSrc1Color,
        17 => BlendFactor::Src1Alpha,
        18 => BlendFactor::InvSrc1Alpha,
        19 => BlendFactor::ConstantAlpha,
        20 => BlendFactor::OneMinusConstantAlpha,
        other => BlendFactor::Other(other),
    }
}

/// A blend combine function - the `CombFunc` enum `CB_BLEND0_CONTROL`'s colour and alpha combines
/// select.
///
/// How the multiplied source and destination are combined. Cited from oops-mesa
/// `src/amd/registers/gfx103.json` (`CombFunc`: `COMB_DST_PLUS_SRC` 0 .. `COMB_DST_MINUS_SRC` 4);
/// the field is three bits and `5..8` are reserved, carried as [`CombineFunc::Other`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CombineFunc {
    /// `dst + src` (additive blend).
    DstPlusSrc,
    /// `src - dst`.
    SrcMinusDst,
    /// `min(dst, src)`.
    MinDstSrc,
    /// `max(dst, src)`.
    MaxDstSrc,
    /// `dst - src`.
    DstMinusSrc,
    /// A reserved encoding (`5..8`), kept raw rather than guessed.
    Other(u32),
}

/// Decodes a three-bit `CombFunc` field into its combine function.
#[must_use]
pub fn decode_combine_func(field: u32) -> CombineFunc {
    match field & 0x7 {
        0 => CombineFunc::DstPlusSrc,
        1 => CombineFunc::SrcMinusDst,
        2 => CombineFunc::MinDstSrc,
        3 => CombineFunc::MaxDstSrc,
        4 => CombineFunc::DstMinusSrc,
        other => CombineFunc::Other(other),
    }
}

/// `CB_BLEND0_CONTROL`, a context register at index `0xA1E0`: `src/amd/registers/gfx103.json` in
/// oops-mesa maps it at byte `165760` and defines its fields.
pub(crate) const CB_BLEND0_CONTROL: u32 = 0xA1E0;

/// The colour-blend state for colour target zero, decoded from `CB_BLEND0_CONTROL`.
///
/// A source and destination factor and a combine function for colour, the same three for alpha, and
/// the flags that turn blending on and let alpha use its own set. What a host needs to build a
/// colour blend attachment. It is colour target zero only; targets `1..8` have their own
/// `CB_BLENDn_CONTROL`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlendControl {
    /// `COLOR_SRCBLEND` (bits 0:4): the source factor for colour.
    pub color_src: BlendFactor,
    /// `COLOR_COMB_FCN` (bits 5:7): how colour source and destination are combined.
    pub color_combine: CombineFunc,
    /// `COLOR_DESTBLEND` (bits 8:12): the destination factor for colour.
    pub color_dst: BlendFactor,
    /// `ALPHA_SRCBLEND` (bits 16:20): the source factor for alpha.
    pub alpha_src: BlendFactor,
    /// `ALPHA_COMB_FCN` (bits 21:23): how alpha source and destination are combined.
    pub alpha_combine: CombineFunc,
    /// `ALPHA_DESTBLEND` (bits 24:28): the destination factor for alpha.
    pub alpha_dst: BlendFactor,
    /// `SEPARATE_ALPHA_BLEND` (bit 29): alpha uses its own factors rather than colour's.
    pub separate_alpha_blend: bool,
    /// `ENABLE` (bit 30): blending runs for this target.
    pub enable: bool,
    /// `DISABLE_ROP3` (bit 31): the raster-op-3 path is off.
    pub disable_rop3: bool,
}

/// Decodes `CB_BLEND0_CONTROL` into its colour and alpha blend state.
#[must_use]
pub fn decode_blend_control(value: u32) -> BlendControl {
    BlendControl {
        color_src: decode_blend_factor(value),
        color_combine: decode_combine_func(value >> 5),
        color_dst: decode_blend_factor(value >> 8),
        alpha_src: decode_blend_factor(value >> 16),
        alpha_combine: decode_combine_func(value >> 21),
        alpha_dst: decode_blend_factor(value >> 24),
        separate_alpha_blend: (value >> 29) & 1 != 0,
        enable: (value >> 30) & 1 != 0,
        disable_rop3: (value >> 31) & 1 != 0,
    }
}

/// The blend state for colour target zero a submission set, from the live value of
/// `CB_BLEND0_CONTROL`.
///
/// [`None`] when the stream never sets it. Most-recent-write-wins.
#[must_use]
pub fn blend_control_at(writes: &[RegisterWrite]) -> Option<BlendControl> {
    let value = writes
        .iter()
        .rev()
        .find(|write| write.register == CB_BLEND0_CONTROL)?
        .value;
    Some(decode_blend_control(value))
}

/// The register that carries colour buffer zero's tiling and layout attributes.
///
/// `CB_COLOR0_ATTRIB3`, a context register at index `0xA3B8`: `src/amd/registers/gfx103.json` in
/// oops-mesa maps it at byte `167648` = `0x28EE0`, context dword `0x3B8`.
const CB_COLOR0_ATTRIB3: u32 = 0xA3B8;

/// `COLOR_SW_MODE` value for a linear (un-swizzled) surface: `ADDR_SW_LINEAR`
/// (`src/amd/addrlib/inc/addrtypes.h:227` in oops-mesa, `AddrSwizzleMode`).
const ADDR_SW_LINEAR: u32 = 0;

/// `COLOR_SW_MODE` value for the 64KB_R_X tiling [`crate::tiling`] detiles: `ADDR_SW_64KB_R_X`
/// (`src/amd/addrlib/inc/addrtypes.h:254` in oops-mesa, `AddrSwizzleMode`).
const ADDR_SW_64KB_R_X: u32 = 27;

/// `COLOR_SW_MODE` value for `ADDR_SW_4KB_D_X` (`src/amd/addrlib/inc/addrtypes.h` in oops-mesa,
/// `AddrSwizzleMode`).
const ADDR_SW_4KB_D_X: u32 = 22;

/// The tiling (swizzle) mode of a surface - a colour target or a texture.
///
/// Only the two modes orbistoun acts on are named: linear and 64KB_R_X. Any other is carried by its
/// raw five-bit value, so it is refused downstream rather than read with the wrong swizzle (D010).
/// The values are `AddrSwizzleMode` (`src/amd/addrlib/inc/addrtypes.h:225` in oops-mesa), shared by
/// `COLOR_SW_MODE` and a texture's `SQ_IMG_RSRC_WORD3.SW_MODE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwizzleMode {
    /// No swizzle; pixels are row-major (`ADDR_SW_LINEAR`).
    Linear,
    /// 64KB_R_X, the mode [`crate::tiling`] detiles (`ADDR_SW_64KB_R_X`).
    Tiled64KbRX,
    /// 4KB_D_X (`ADDR_SW_4KB_D_X`), which radeonsi gives a small colour surface.
    Tiled4KbDX,
    /// A mode orbistoun does not model, carried by its raw five-bit swizzle-mode value.
    Other(u32),
}

/// Decodes a five-bit `SW_MODE` field into a [`SwizzleMode`].
///
/// The field is the raw value a `COLOR_SW_MODE` or `SQ_IMG_RSRC_WORD3.SW_MODE` carries once the
/// caller has shifted it out of its register. Values are `AddrSwizzleMode`: linear `0`, 64KB_R_X
/// `27`.
#[must_use]
pub fn decode_swizzle_mode(field: u32) -> SwizzleMode {
    match field & 0x1F {
        ADDR_SW_LINEAR => SwizzleMode::Linear,
        ADDR_SW_64KB_R_X => SwizzleMode::Tiled64KbRX,
        ADDR_SW_4KB_D_X => SwizzleMode::Tiled4KbDX,
        other => SwizzleMode::Other(other),
    }
}

/// Decodes `CB_COLOR0_ATTRIB3`'s `COLOR_SW_MODE` (bits 18:14) into a swizzle mode.
///
/// The field bits are from `src/amd/registers/gfx103.json` in oops-mesa; a 64x64 point-draw
/// capture's `0x08c6c000` decodes to `27` = 64KB_R_X.
#[must_use]
pub fn decode_colour_swizzle_mode(attrib3: u32) -> SwizzleMode {
    decode_swizzle_mode((attrib3 >> 14) & 0x1F)
}

/// The colour target's tiling mode, from the live value of `CB_COLOR0_ATTRIB3` among the writes.
///
/// [`None`] when the stream never sets it: an unknown tiling is read as neither linear nor tiled.
/// Most-recent-write-wins.
#[must_use]
pub fn colour_swizzle_mode_at(writes: &[RegisterWrite]) -> Option<SwizzleMode> {
    let value = writes
        .iter()
        .rev()
        .find(|write| write.register == CB_COLOR0_ATTRIB3)?
        .value;
    Some(decode_colour_swizzle_mode(value))
}

/// `PA_CL_VPORT_XSCALE`, the first of the four viewport-transform registers `XSCALE`, `XOFFSET`,
/// `YSCALE`, `YOFFSET`, which `gfx103.json` maps at bytes `164924`..`164936`, dwords
/// `0xA10F`..`0xA112`.
const PA_CL_VPORT_XSCALE: u32 = 0xA10F;
/// `PA_CL_VTE_CNTL` (`gfx103.json`, byte `165912`, dword `0xA206`): bits 0-3 enable the x/y scale
/// and offset.
const PA_CL_VTE_CNTL: u32 = 0xA206;
/// `VPORT_X_SCALE_ENA` through `VPORT_Y_OFFSET_ENA`.
const VTE_XY_ENABLES: u32 = 0xF;
/// `VPORT_Z_SCALE_ENA`, `PA_CL_VTE_CNTL` bit 4 (`gfx103.json:13363`).
const VTE_Z_SCALE_ENA: u32 = 1 << 4;
/// `VPORT_Z_OFFSET_ENA`, bit 5.
const VTE_Z_OFFSET_ENA: u32 = 1 << 5;
/// `PA_CL_VPORT_ZSCALE` (`gfx103.json:3600`, byte `164940`, dword `0xA113`); `PA_CL_VPORT_ZOFFSET`
/// follows it at `0xA114`.
const PA_CL_VPORT_ZSCALE: u32 = 0xA113;
/// `PA_CL_CLIP_CNTL` (`gfx103.json:4653`, byte `165904`, dword `0xA204`).
const PA_CL_CLIP_CNTL: u32 = 0xA204;
/// `DX_CLIP_SPACE_DEF`, `PA_CL_CLIP_CNTL` bit 19 (`gfx103.json:13275`): set, clip-space z runs
/// `0..w`; clear, `-w..w` as GL's does.
const DX_CLIP_SPACE_DEF: u32 = 1 << 19;

/// The viewport transform a draw's clip-space positions are mapped to the target with:
/// `x = x_scale * ndc_x + x_offset`, `y = y_scale * ndc_y + y_offset`, in the target's pixels, rows
/// growing down.
///
/// The open-toolchain GL context writes `YSCALE = -height / 2`, since GL's NDC `+y` is up
/// (`gl_internal.h`, `gl_compute_vport` in oops-sdk). A host ignoring the sign draws upside down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportTransform {
    /// `PA_CL_VPORT_XSCALE`.
    pub x_scale: f32,
    /// `PA_CL_VPORT_XOFFSET`.
    pub x_offset: f32,
    /// `PA_CL_VPORT_YSCALE`.
    pub y_scale: f32,
    /// `PA_CL_VPORT_YOFFSET`.
    pub y_offset: f32,
    /// How clip-space z is clipped and becomes depth.
    pub depth: DepthMapping,
}

/// How a draw's clip-space z is clipped and mapped to depth: the range it is clipped to, then
/// `depth = z_scale * ndc_z + z_offset`.
///
/// A GL context clips z to `-w..w` and writes `ZSCALE = (far - near) / 2`, `ZOFFSET = (far + near)
/// / 2` (`gl_draw.c`, the `glDepthRange` arm, in oops-sdk). A host clipping to `0..w` instead drops
/// everything nearer than the middle of the depth range: Bugdom's HUD, drawn at the near plane,
/// vanished whole.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DepthMapping {
    /// Clip-space z runs `-w..w`, GL's convention - `PA_CL_CLIP_CNTL.DX_CLIP_SPACE_DEF` clear.
    /// `false` is `0..w`.
    pub negative_one_to_one: bool,
    /// `PA_CL_VPORT_ZSCALE` when enabled; one otherwise.
    pub z_scale: f32,
    /// `PA_CL_VPORT_ZOFFSET` when enabled; zero otherwise.
    pub z_offset: f32,
}

impl DepthMapping {
    /// Clip z `0..w` taken as depth unchanged: what a stream that sets none of the registers gets.
    pub const IDENTITY: Self = Self {
        negative_one_to_one: false,
        z_scale: 1.0,
        z_offset: 0.0,
    };
}

/// The viewport transform in force at packet `before`, from the last write of each of the four
/// `PA_CL_VPORT_*` registers in an earlier packet.
///
/// `None` when any of the four was never written, or when `PA_CL_VTE_CNTL` was written without all
/// four x/y enables, since the hardware then does not apply them and that case is not modelled.
/// Both SDK draw paths write `0x43f`.
#[must_use]
pub fn viewport_transform_at(writes: &[RegisterWrite], before: u32) -> Option<ViewportTransform> {
    viewport_transform_from(|register: u32| {
        writes
            .iter()
            .rev()
            .find(|w| w.register == register && w.packet_offset < before)
            .map(|w| w.value)
    })
}

/// Every `PA_CL_VTE_CNTL` field this reads: the six scale and offset enables (bits 0-5) and
/// `VTX_XY_FMT`, `VTX_Z_FMT` and `VTX_W0_FMT` (bits 8-10, `gfx103.json:13365-13367`).
const VTE_FIELDS: u32 = 0x73F;
/// The value radeonsi gives a window-space shader (`si_state_shaders.cpp:1321-1322`): x, y and z
/// pre-divided, no scale or offset, and `VTX_W0_FMT` clear - Gallium's `VS_WINDOW_SPACE_POSITION`,
/// whose fourth component is `1/W` taken as is (`docs/gallium/tgsi.rst:3705-3711`).
const VTE_WINDOW_SPACE: u32 = 0x300;

/// The space a draw's positions are in, from `PA_CL_VTE_CNTL`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionSpace {
    /// Clip space, mapped through the viewport transform: the x/y terms are on, or the stream
    /// never wrote the register.
    Clip,
    /// Window space, in radeonsi's form (D731).
    Window,
    /// The x/y terms off in some other form, with the value that asked for it.
    Unmodelled(u32),
}

/// The space the positions of a draw are in, reading each register's value through `last` - a
/// [`RegisterSweep::latest`] for a caller walking draws in order.
pub fn position_space(mut last: impl FnMut(u32) -> Option<u32>) -> PositionSpace {
    match last(PA_CL_VTE_CNTL) {
        Some(value) if value & VTE_XY_ENABLES != VTE_XY_ENABLES => {
            if value & VTE_FIELDS == VTE_WINDOW_SPACE {
                PositionSpace::Window
            } else {
                PositionSpace::Unmodelled(value)
            }
        }
        _ => PositionSpace::Clip,
    }
}

/// [`viewport_transform_at`], reading each register's value through `last` - a
/// [`RegisterSweep::latest`] for a caller walking draws in order.
pub fn viewport_transform_from(
    mut last: impl FnMut(u32) -> Option<u32>,
) -> Option<ViewportTransform> {
    if last(PA_CL_VTE_CNTL).is_some_and(|value| value & VTE_XY_ENABLES != VTE_XY_ENABLES) {
        return None;
    }
    let vte = last(PA_CL_VTE_CNTL);
    // A z term is applied when its enable is set, or when the stream never wrote the control
    // register, as the x/y terms are. A z register the stream never wrote leaves that term as it
    // was before the mapping was modelled: z taken unchanged.
    let enabled = |bit: u32| vte.is_none_or(|value| value & bit != 0);
    let z_scale = if enabled(VTE_Z_SCALE_ENA) {
        last(PA_CL_VPORT_ZSCALE).map_or(1.0, f32::from_bits)
    } else {
        1.0
    };
    let z_offset = if enabled(VTE_Z_OFFSET_ENA) {
        last(PA_CL_VPORT_ZSCALE + 1).map_or(0.0, f32::from_bits)
    } else {
        0.0
    };
    let negative_one_to_one =
        last(PA_CL_CLIP_CNTL).is_some_and(|value| value & DX_CLIP_SPACE_DEF == 0);
    Some(ViewportTransform {
        x_scale: f32::from_bits(last(PA_CL_VPORT_XSCALE)?),
        x_offset: f32::from_bits(last(PA_CL_VPORT_XSCALE + 1)?),
        y_scale: f32::from_bits(last(PA_CL_VPORT_XSCALE + 2)?),
        y_offset: f32::from_bits(last(PA_CL_VPORT_XSCALE + 3)?),
        depth: DepthMapping {
            negative_one_to_one,
            z_scale,
            z_offset,
        },
    })
}

/// `CB_COLOR0_INFO`, a context register: `gfx103.json` maps it at byte `167024`, dword `0xA31C`.
const CB_COLOR0_INFO: u32 = 0xA31C;

/// `ColorFormat` `COLOR_8_8_8_8` (`gfx103.json`, enum `ColorFormat`).
pub const COLOR_8_8_8_8: u32 = 10;
/// `SurfaceNumber` `NUMBER_UNORM` (`gfx103.json`, enum `SurfaceNumber`).
pub const NUMBER_UNORM: u32 = 0;

/// Which memory byte each of a four-channel colour target's shader outputs lands in -
/// `CB_COLOR0_INFO.COMP_SWAP`.
///
/// The two orders named are the ones Mesa gives a four-channel format (`ac_formats.c:614-619`):
/// `SWAP_STD` is `XYZW` (memory holds R, G, B, A) and `SWAP_ALT` is `ZYXW` (memory holds B, G, R,
/// A), the order the open-toolchain GL context's display targets use (`gl_draw.c` in oops-sdk). The
/// reversed orders are carried raw and refused where a byte order is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentSwap {
    /// `SWAP_STD` (0): memory order R, G, B, A.
    Standard,
    /// `SWAP_ALT` (1): memory order B, G, R, A.
    Alternate,
    /// `SWAP_STD_REV` (2) or `SWAP_ALT_REV` (3), by raw value.
    Other(u32),
}

/// Colour target zero's element layout: `CB_COLOR0_INFO`'s `FORMAT` (bits 2-6), `NUMBER_TYPE`
/// (8-10) and `COMP_SWAP` (11-12), field bits from `oops-mesa src/amd/registers/gfx103.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColourTargetFormat {
    /// The `ColorFormat` code.
    pub format: u32,
    /// The `SurfaceNumber` code.
    pub number_type: u32,
    /// The component order in memory.
    pub swap: ComponentSwap,
}

impl ColourTargetFormat {
    /// Whether this is a four-byte `8_8_8_8` `UNORM` target in an order [`Self::swap`] names - the
    /// layout a rendered `Rgba8` frame can be written into exactly.
    #[must_use]
    pub const fn is_rgba8_class(&self) -> bool {
        self.format == COLOR_8_8_8_8
            && self.number_type == NUMBER_UNORM
            && matches!(
                self.swap,
                ComponentSwap::Standard | ComponentSwap::Alternate
            )
    }
}

/// Decodes a `CB_COLOR0_INFO` value. The SDK's measured primitive-draw value `0x000180a8` decodes
/// to `8_8_8_8`, `UNORM`, `SWAP_STD`.
#[must_use]
pub const fn decode_colour_target_format(info: u32) -> ColourTargetFormat {
    ColourTargetFormat {
        format: (info >> 2) & 0x1F,
        number_type: (info >> 8) & 0x7,
        swap: match (info >> 11) & 0x3 {
            0 => ComponentSwap::Standard,
            1 => ComponentSwap::Alternate,
            other => ComponentSwap::Other(other),
        },
    }
}

/// Colour target zero's element layout from the live `CB_COLOR0_INFO` among the writes; [`None`]
/// when the stream never set it. Most-recent-write-wins.
#[must_use]
pub fn colour_target_format_at(writes: &[RegisterWrite]) -> Option<ColourTargetFormat> {
    let value = writes
        .iter()
        .rev()
        .find(|write| write.register == CB_COLOR0_INFO)?
        .value;
    Some(decode_colour_target_format(value))
}

/// How many distinct colour target zero base addresses a stream's draws drew into, counting the
/// base left in force after them - the one a frame is written back to. `draws` are the draw
/// packets' byte offsets. A submission whose draws are carried out together and written back as one
/// frame must have drawn into one target, so more than one is a refusal; a base overwritten before
/// any draw used it is not a target.
#[must_use]
pub fn colour_target_bases_in(writes: &[RegisterWrite], draws: &[u32]) -> usize {
    // One sweep over the base writes and the draws, each in offset order: the base in force at a
    // draw is the last write before it. A stream holds thousands of writes and hundreds of draws,
    // so asking per draw is what a profile of a menu frame found (9% of the guest thread).
    let mut bases_written: Vec<(u32, u32)> = writes
        .iter()
        .filter(|write| write.register == CB_COLOR0_BASE)
        .map(|write| (write.packet_offset, write.value))
        .collect();
    let left_in_force = bases_written.last().map(|&(_, value)| value);
    bases_written.sort_by_key(|&(offset, _)| offset);
    let mut draws = draws.to_vec();
    draws.sort_unstable();
    let mut bases: Vec<u32> = left_in_force.into_iter().collect();
    let mut next = 0;
    let mut in_force = None;
    for draw in draws {
        while let Some(&(offset, value)) = bases_written.get(next)
            && offset < draw
        {
            in_force = Some(value);
            next += 1;
        }
        bases.extend(in_force);
    }
    bases.sort_unstable();
    bases.dedup();
    bases.len()
}

/// The absolute dword index of `VGT_GS_OUT_PRIM_TYPE`.
///
/// From `src/amd/registers/gfx103.json` in oops-mesa (`"map": {"at": 166508}`; `166508 / 4` =
/// `0xA29B`), a context-space register. The point-draw capture (`agc-primitive-draw-fw1240`) writes
/// `0` (POINTLIST); the triangle and gl-cube captures write `2` (TRISTRIP).
const VGT_GS_OUT_PRIM_TYPE: u32 = 0xA29B;

/// The primitive a draw's geometry produces, from `VGT_GS_OUT_PRIM_TYPE.OUTPRIM_TYPE`.
///
/// The output primitive type - what the geometry stage emits and the rasteriser assembles. It
/// tells a point draw from a triangle one where the input-assembly `VGT_PRIMITIVE_TYPE` reads
/// `TRILIST` for both. Only values with a meaning here are named; others are carried raw and
/// refused downstream (D010). `RectangleList` has no graphics-primitive analogue, so a consumer
/// refuses it by name. Values are `VGT_GS_OUTPRIM_TYPE` (`src/amd/registers/gfx103.json` in
/// oops-mesa): POINTLIST 0, LINESTRIP 1, TRISTRIP 2, RECTLIST 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveTopology {
    /// A list of points (POINTLIST).
    PointList,
    /// A connected strip of lines (LINESTRIP).
    LineStrip,
    /// A connected strip of triangles (TRISTRIP).
    TriangleStrip,
    /// A list of screen-space rectangles (RECTLIST) - no direct graphics-primitive analogue here.
    RectangleList,
    /// A value not in `VGT_GS_OUTPRIM_TYPE`, carried by its raw six-bit field.
    Other(u32),
}

impl PrimitiveTopology {
    /// A short name for a report - what a reader sees when a submission's topology is surfaced.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::PointList => "point list".to_owned(),
            Self::LineStrip => "line strip".to_owned(),
            Self::TriangleStrip => "triangle strip".to_owned(),
            Self::RectangleList => "rectangle list".to_owned(),
            Self::Other(value) => format!("VGT_GS_OUTPRIM_TYPE {value}"),
        }
    }
}

/// Decodes `VGT_GS_OUT_PRIM_TYPE.OUTPRIM_TYPE` (bits 5:0) into a topology.
///
/// The field bits and values are from `src/amd/registers/gfx103.json` in oops-mesa.
#[must_use]
pub fn decode_primitive_topology(field: u32) -> PrimitiveTopology {
    match field & 0x3F {
        0 => PrimitiveTopology::PointList,
        1 => PrimitiveTopology::LineStrip,
        2 => PrimitiveTopology::TriangleStrip,
        3 => PrimitiveTopology::RectangleList,
        other => PrimitiveTopology::Other(other),
    }
}

/// The primitive a draw produces, from the live value of `VGT_GS_OUT_PRIM_TYPE` among the writes.
///
/// [`None`] when the stream never sets it: an unknown topology is not assumed a triangle list.
/// Most-recent-write-wins.
#[must_use]
pub fn primitive_topology_at(writes: &[RegisterWrite]) -> Option<PrimitiveTopology> {
    let value = writes
        .iter()
        .rev()
        .find(|write| write.register == VGT_GS_OUT_PRIM_TYPE)?
        .value;
    Some(decode_primitive_topology(value))
}

/// The absolute dword index of `VGT_SHADER_STAGES_EN`.
///
/// From `src/amd/registers/gfx103.json` in oops-mesa (`"map": {"at": 166740}`; `166740 / 4` =
/// `0xA2D5`), a context-space register. `GS_W32_EN` is its bit 22: the primitive shader - the
/// host's mesh stage - is thirty-two lanes wide.
const VGT_SHADER_STAGES_EN: u32 = 0xA2D5;
const GS_W32_EN: u32 = 1 << 22;

/// The absolute dword index of `SPI_PS_IN_CONTROL`.
///
/// From `src/amd/registers/gfx103.json` in oops-mesa (`"map": {"at": 165592}`; `165592 / 4` =
/// `0xA1B6`). `PS_W32_EN` is its bit 15: the pixel shader is thirty-two lanes wide.
const SPI_PS_IN_CONTROL: u32 = 0xA1B6;
const PS_W32_EN: u32 = 1 << 15;

/// Which of a draw's stages run thirty-two lanes wide.
///
/// The instruction encodings are the same at either width, so the hardware is told in these two
/// registers. A clear bit, or a register the stream never wrote (reset value zero), is the
/// sixty-four-lane wave.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WaveWidths {
    /// The primitive shader (the host's mesh stage) is thirty-two lanes wide.
    pub primitive_w32: bool,
    /// The pixel shader is thirty-two lanes wide.
    pub pixel_w32: bool,
}

/// The stages' wave widths, from the live values of `VGT_SHADER_STAGES_EN` and `SPI_PS_IN_CONTROL`
/// among the writes. Most-recent-write-wins.
#[must_use]
pub fn wave_widths_at(writes: &[RegisterWrite]) -> WaveWidths {
    let latest = |register| {
        writes
            .iter()
            .rev()
            .find(|write| write.register == register)
            .map_or(0, |write| write.value)
    };
    WaveWidths {
        primitive_w32: latest(VGT_SHADER_STAGES_EN) & GS_W32_EN != 0,
        pixel_w32: latest(SPI_PS_IN_CONTROL) & PS_W32_EN != 0,
    }
}

/// An image resource descriptor - a "T#" - decoded from its eight dwords.
///
/// The concrete-value counterpart of the sampled image the translator emits as arithmetic
/// (`orbistoun_translate::model`, `IMAGE_DESCRIPTOR_REGISTERS`), as [`BufferDescriptor`] is of a
/// V#. Per the GFX10 layout (`src/amd/registers/gfx10-rsrc.json` and
/// `ac_descriptors.c:ac_build_gfx10_texture_descriptor` in oops-mesa): base in dword 0 (256-byte
/// units) plus the low byte of dword 1; format in dword 1 bits 28:20; width minus one split across
/// dword 1 bits 31:30 and dword 2 bits 11:0; height minus one in dword 2 bits 27:14; tiling mode in
/// dword 3 bits 24:20 (`SQ_IMG_RSRC_WORD3.SW_MODE`), the same `AddrSwizzleMode` a colour target
/// carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageDescriptor {
    /// Byte address of the texture's pixels.
    pub base: u64,
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// The image format, as the GFX10 format code. Reported, not interpreted: a host maps it to its
    /// own format and refuses one it does not recognise.
    pub format: u32,
    /// The tiling mode - which swizzle, if any, the pixels are stored under. `Tiled64KbRX` is the
    /// one [`crate::tiling`] detiles; `Linear` reads straight; `Other` is a mode to refuse rather
    /// than read with the wrong swizzle.
    pub tiling: SwizzleMode,
    /// The surface's pipe-bank XOR, which radeonsi ORs into dword 0's low bits below the block's
    /// alignment (`ac_descriptors.c:767`, `desc[0] |= surf->tile_swizzle`); zero for a linear one.
    pub pipe_bank_xor: u8,
    /// Levels in the surface's mip chain, `SQ_IMG_RSRC_WORD5.MAX_MIP` (bits 7:4) plus one: what
    /// places each level (`ac_descriptors.c:546`, `num_levels - 1`).
    pub levels: u32,
    /// The first level the view reads, `SQ_IMG_RSRC_WORD3.BASE_LEVEL` (bits 15:12).
    pub base_level: u32,
    /// The view's last level, `SQ_IMG_RSRC_WORD3.LAST_LEVEL` (bits 19:16).
    pub last_level: u32,
    /// Delta colour compression, when `SQ_IMG_RSRC_WORD6.COMPRESSION_EN` (bit 21) is set: the keys
    /// at `META_DATA_ADDRESS_LO` (word 6 bits 31:24, address bits 15:8) and word 7 (address bits
    /// 47:16), laid out pipe-aligned when `META_PIPE_ALIGNED` (bit 19) says so
    /// (`ac_descriptors.c:748-765`).
    pub compression: Option<crate::dcc::Dcc>,
}

/// The row pitch, in texels, of a linear 2D image of `bytes_per_texel`-byte texels whose
/// descriptor's dword 4 is `word4`.
///
/// Zero there means the pitch the hardware derives from the width: rows aligned to 256 bytes - 64
/// four-byte texels, 256 one-byte ones (addrlib's `ADDR_SW_LINEAR` pitch, `256 / elementBytes`,
/// `gfx10addrlib.cpp:5079`; oops-sdk writes it zero for every 2D texture and relies on exactly that
/// on hardware, `gl_state.c:4334`). Anything else is the custom pitch less one: the low bits in
/// `DEPTH` (12:0) and the top bit in `PITCH_MSB` (13) (`gfx10-rsrc.json:403-404`).
#[must_use]
pub const fn linear_pitch(word4: u32, width: u32, bytes_per_texel: u32) -> u32 {
    let field = word4 & 0x3FFF;
    if field == 0 {
        width.next_multiple_of(256 / bytes_per_texel)
    } else {
        field + 1
    }
}

impl ImageDescriptor {
    /// The level the view reads first, as the colour target the same surface would be drawn
    /// through: its first byte, extent and layout, and for a level in the mip tail its origin in
    /// the tail's block - or `None` for a tiling, or a level's place, not modelled.
    #[must_use]
    pub fn base_level_surface(&self) -> Option<ColourTarget> {
        chain_level(
            (self.base, self.pipe_bank_xor),
            (self.width, self.height),
            (self.levels, self.base_level),
            crate::tiling::SurfaceLayout::of(self.tiling)?,
        )
    }
}

/// How a texture coordinate outside `[0, 1]` is brought back: `SQ_IMG_SAMP_WORD0.CLAMP_X/Y`
/// (`gfx10-rsrc.json:462-463`), values as radeonsi's `si_tex_wrap` writes them and the SDK's GL
/// layer uses on hardware (oops-sdk `gl_state.c:gl_hw_wrap`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureWrap {
    /// `SQ_TEX_WRAP` (0): repeat.
    Repeat,
    /// `SQ_TEX_MIRROR` (1): mirrored repeat.
    Mirror,
    /// `SQ_TEX_CLAMP_LAST_TEXEL` (2): clamp to the edge texel.
    ClampToEdge,
}

/// A texture filter: `SQ_IMG_SAMP_WORD2.XY_MAG_FILTER` / `XY_MIN_FILTER` (`gfx10-rsrc.json:490-491`),
/// 0 point and 1 bilinear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureFilter {
    /// Point sampling.
    Nearest,
    /// Bilinear.
    Linear,
}

/// Between mip levels: `SQ_IMG_SAMP_WORD2.MIP_FILTER` (`gfx10-rsrc.json:493`), 0 none, 1 point, 2
/// linear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MipFilter {
    /// The base level only.
    None,
    /// The nearest level.
    Nearest,
    /// Between the two nearest levels.
    Linear,
}

/// How a texture is sampled, from its sampler descriptor (an S#).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureSampling {
    /// The wrap across and down.
    pub wrap: [TextureWrap; 2],
    /// Magnifying.
    pub magnify: TextureFilter,
    /// Minifying.
    pub minify: TextureFilter,
    /// Between levels.
    pub mip: MipFilter,
}

impl TextureSampling {
    /// Point sampling clamped to the edge: what a texture no sampler names is read with, such as
    /// one only fetched by index.
    pub const CLAMPED_POINT: Self = Self {
        wrap: [TextureWrap::ClampToEdge; 2],
        magnify: TextureFilter::Nearest,
        minify: TextureFilter::Nearest,
        mip: MipFilter::Nearest,
    };
}

impl Default for TextureSampling {
    /// [`Self::CLAMPED_POINT`].
    fn default() -> Self {
        Self::CLAMPED_POINT
    }
}

/// Decodes a sampler descriptor's four words into how it samples, or names the field it cannot
/// honour exactly: a border or half-border clamp (which needs the border colour), a mirror-once
/// mode, an anisotropic filter, or a reserved value. Only the wrap across and down and the three
/// filters are read; a 2D texture has no third coordinate.
pub fn decode_sampler_descriptor(words: [u32; 4]) -> Result<TextureSampling, &'static str> {
    let wrap = |value: u32| match value {
        0 => Ok(TextureWrap::Repeat),
        1 => Ok(TextureWrap::Mirror),
        2 => Ok(TextureWrap::ClampToEdge),
        _ => Err(concat!(
            "the sampler clamps to a border or mirrors once, which needs its border colour ",
            "or a mode with no exact host form"
        )),
    };
    let filter = |value: u32| match value {
        0 => Ok(TextureFilter::Nearest),
        1 => Ok(TextureFilter::Linear),
        _ => Err("the sampler filters anisotropically, which is not reproduced"),
    };
    let mip = match (words[2] >> 26) & 0x3 {
        0 => MipFilter::None,
        1 => MipFilter::Nearest,
        2 => MipFilter::Linear,
        _ => return Err("the sampler's mip filter is a reserved value"),
    };
    Ok(TextureSampling {
        wrap: [wrap(words[0] & 0x7)?, wrap((words[0] >> 3) & 0x7)?],
        magnify: filter((words[2] >> 20) & 0x3)?,
        minify: filter((words[2] >> 22) & 0x3)?,
        mip,
    })
}

/// A scissor rectangle - the region a guest restricts rasterisation to - decoded from the
/// `GENERIC_SCISSOR` top-left and bottom-right register pair.
///
/// The application scissor a `SetViewport` carries: a draw paints only inside it. Its coordinates
/// are non-negative pixels in the target, so all four fields are unsigned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scissor {
    /// Left edge, in pixels.
    pub x: u32,
    /// Top edge, in pixels.
    pub y: u32,
    /// Width in pixels: bottom-right minus top-left, clamped at zero for an empty rectangle.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// `PA_SC_GENERIC_SCISSOR_TL` and `_BR` - the application scissor's corners.
///
/// Context registers `0xA090` and `0xA091`. The obSCEne draw oracle (`166-agc/primitive-draw`)
/// writes `0x80000000`/`0x00400040` here for its 64x64 frame, which decodes to `(0,0)`-`(64,64)`.
/// The generic scissor is the application-controlled one, as opposed to the screen, window and
/// per-viewport scissors.
const PA_SC_GENERIC_SCISSOR_TL: u32 = 0xA090;
/// See [`PA_SC_GENERIC_SCISSOR_TL`].
const PA_SC_GENERIC_SCISSOR_BR: u32 = 0xA091;

/// Decodes a scissor rectangle from its top-left and bottom-right register values.
///
/// The layout is `PA_SC_WINDOW_SCISSOR_TL`/`_BR` (`src/amd/registers/gfx10.json` in oops-mesa),
/// which the generic scissor shares: x in bits 14:0, y in bits 30:16, and on the top-left a
/// `WINDOW_OFFSET_DISABLE` flag at bit 31 that is masked off. Width and height are the corners'
/// difference, clamped at zero.
#[must_use]
pub fn decode_scissor(top_left: u32, bottom_right: u32) -> Scissor {
    let (tl_x, tl_y) = (top_left & 0x7FFF, (top_left >> 16) & 0x7FFF);
    let (br_x, br_y) = (bottom_right & 0x7FFF, (bottom_right >> 16) & 0x7FFF);
    Scissor {
        x: tl_x,
        y: tl_y,
        width: br_x.saturating_sub(tl_x),
        height: br_y.saturating_sub(tl_y),
    }
}

/// The scissor from `latest`, the value each register holds at some point in a stream, or
/// [`None`] when only one corner or neither is set: the missing corner is not guessed (D010). What
/// a draw is restricted to is the scissor in force at it.
#[must_use]
pub fn scissor_from(mut latest: impl FnMut(u32) -> Option<u32>) -> Option<Scissor> {
    Some(decode_scissor(
        latest(PA_SC_GENERIC_SCISSOR_TL)?,
        latest(PA_SC_GENERIC_SCISSOR_BR)?,
    ))
}

/// The scissor from the live `GENERIC_SCISSOR` writes, or [`None`] when the stream set only one
/// corner or neither: the missing corner is not guessed (D010). The most recent write to each wins.
#[must_use]
pub fn scissor_at(writes: &[RegisterWrite]) -> Option<Scissor> {
    let live = |register: u32| {
        writes
            .iter()
            .rev()
            .find(|write| write.register == register)
            .map(|write| write.value)
    };
    Some(decode_scissor(
        live(PA_SC_GENERIC_SCISSOR_TL)?,
        live(PA_SC_GENERIC_SCISSOR_BR)?,
    ))
}

/// Decodes an image resource descriptor from its eight dwords, in register order.
///
/// The bit positions are those [`ImageDescriptor`] documents. The first three dwords carry the
/// base, format and extent and dword 3 the tiling mode; mip levels and array bounds are not
/// modelled.
#[must_use]
pub fn decode_image_descriptor(words: [u32; 8]) -> ImageDescriptor {
    // dword 1 (`mid`) carries the base's high byte, the format and the width's low bits; dword 2
    // (`extent`) the width's high bits and the height; dword 3 (`tiling_word`) the tiling mode.
    let [
        base_low,
        mid,
        extent,
        tiling_word,
        _,
        levels_word,
        meta_word,
        meta_high,
    ] = words;
    // Tiling mode: dword 3 bits 24:20 (`SQ_IMG_RSRC_WORD3.SW_MODE`, gfx10-rsrc.json).
    let tiling = decode_swizzle_mode((tiling_word >> 20) & 0x1F);
    // The pipe-bank XOR sits in the 256-byte units below the block's alignment: 64 KiB for
    // `64KB_R_X`, 4 KiB for `4KB_D_X`. A linear surface has none.
    let xor_bits = match tiling {
        SwizzleMode::Tiled64KbRX => 0xFF,
        SwizzleMode::Tiled4KbDX => 0xF,
        _ => 0,
    };
    let base = (u64::from(base_low & !xor_bits) | (u64::from(mid & 0xFF) << 32)) << 8;
    // Width minus one: low two bits in dword 1 bits 31:30, the rest in dword 2 bits 11:0.
    let width = (((mid >> 30) & 0x3) | ((extent & 0xFFF) << 2)) + 1;
    let height = ((extent >> 14) & 0x3FFF) + 1;
    let compression = (meta_word >> 21 & 1 != 0).then(|| crate::dcc::Dcc {
        base: (u64::from(meta_high) << 16) | (u64::from(meta_word >> 24) << 8),
        pipe_aligned: meta_word >> 19 & 1 != 0,
    });
    ImageDescriptor {
        base,
        width,
        height,
        format: (mid >> 20) & 0x1FF,
        tiling,
        pipe_bank_xor: (base_low & xor_bits) as u8,
        levels: (levels_word >> 4 & 0xF) + 1,
        base_level: tiling_word >> 12 & 0xF,
        last_level: tiling_word >> 16 & 0xF,
        compression,
    }
}

#[cfg(test)]
mod tests {

    /// Sampler words as the SDK's GL layer writes them (`gl_state.c`: `cx | cy << 3` in word 0,
    /// `mag << 20 | min << 22 | mip << 26` in word 2) decode to the wrap and filters asked for; a
    /// border clamp and an anisotropic filter are refused rather than approximated.
    #[test]
    fn a_sampler_descriptor_decodes_to_its_wrap_and_filters() {
        use super::{MipFilter, TextureFilter, TextureWrap, decode_sampler_descriptor};
        let words = |cx: u32, cy: u32, mag: u32, min: u32, mip: u32| {
            [
                cx | (cy << 3),
                0,
                (mag << 20) | (min << 22) | (mip << 26),
                0,
            ]
        };
        let repeat_linear = decode_sampler_descriptor(words(0, 1, 1, 1, 2)).expect("decodes");
        assert_eq!(
            repeat_linear.wrap,
            [TextureWrap::Repeat, TextureWrap::Mirror]
        );
        assert_eq!(
            (
                repeat_linear.magnify,
                repeat_linear.minify,
                repeat_linear.mip
            ),
            (
                TextureFilter::Linear,
                TextureFilter::Linear,
                MipFilter::Linear
            )
        );
        let clamped = decode_sampler_descriptor(words(2, 2, 0, 0, 0)).expect("decodes");
        assert_eq!(clamped.wrap, [TextureWrap::ClampToEdge; 2]);
        assert_eq!(clamped.mip, MipFilter::None);
        assert!(
            decode_sampler_descriptor(words(6, 0, 0, 0, 0)).is_err(),
            "border"
        );
        assert!(
            decode_sampler_descriptor(words(0, 4, 0, 0, 0)).is_err(),
            "half border"
        );
        assert!(
            decode_sampler_descriptor(words(0, 0, 2, 0, 0)).is_err(),
            "aniso"
        );
    }

    /// The sweep answers what the whole stream would, forwards, backwards and past the table.
    #[test]
    fn the_sweep_gives_each_registers_latest_earlier_write() {
        use super::{RegisterSweep, RegisterWrite};
        let write = |packet_offset, register, value| RegisterWrite {
            packet_offset,
            register,
            value,
        };
        let writes = [
            write(0, 0xA1E0, 1),
            write(4, 0x2C8C, 2),
            write(8, 0xA1E0, 3),
            write(12, 0x1_0005, 4),
        ];
        let mut sweep = RegisterSweep::new(&writes);
        assert_eq!(
            sweep.latest(0, 0xA1E0),
            None,
            "a write in the packet itself is not before it"
        );
        assert_eq!(sweep.latest(8, 0xA1E0), Some(1));
        assert_eq!(sweep.latest(9, 0xA1E0), Some(3));
        assert_eq!(sweep.latest(9, 0x2C8C), Some(2));
        assert_eq!(sweep.latest(13, 0x1_0005), Some(4));
        // Backwards: the later writes are forgotten, not kept.
        assert_eq!(sweep.latest(5, 0xA1E0), Some(1));
        assert_eq!(sweep.latest(5, 0x1_0005), None);
        let among = sweep.before_among(13, [0x1_0005, 0xA1E0, 0x2C8C]);
        let values: Vec<u32> = among.iter().map(|w| w.value).collect();
        assert_eq!(values, [2, 3, 4], "in the order they were made");
    }

    /// A sweep starts from nothing, whatever the sweep before it on this thread saw: a register the
    /// last stream wrote and this one does not reads unwritten.
    #[test]
    fn a_reused_sweep_table_remembers_nothing_of_the_last_stream() {
        use super::{RegisterSweep, RegisterWrite};
        let write = |packet_offset, register, value| RegisterWrite {
            packet_offset,
            register,
            value,
        };
        let first = [write(0, 0xA1E0, 7), write(4, 0x2C8C, 8)];
        {
            let mut sweep = RegisterSweep::new(&first);
            assert_eq!(sweep.latest(8, 0xA1E0), Some(7));
            assert_eq!(sweep.latest(8, 0x2C8C), Some(8));
        }
        let second = [write(0, 0x2C8C, 9)];
        let mut sweep = RegisterSweep::new(&second);
        assert_eq!(
            sweep.latest(8, 0xA1E0),
            None,
            "the last stream's write is not this one's"
        );
        assert_eq!(sweep.latest(8, 0x2C8C), Some(9));
    }

    /// Each stage's wave width comes from its own bit, and the later write wins. The GL values are
    /// the context's NGG setup in `src/gl/gl_draw.c` in oops-sdk.
    #[test]
    fn wave_widths_read_each_stages_own_bit() {
        use super::{RegisterWrite, WaveWidths, wave_widths_at};
        let write = |register, value| RegisterWrite {
            packet_offset: 0,
            register,
            value,
        };
        assert_eq!(wave_widths_at(&[]), WaveWidths::default());
        let gl = [write(0xA2D5, 0x00c1_2010), write(0xA1B6, 0x0000_8002)];
        assert_eq!(
            wave_widths_at(&gl),
            WaveWidths {
                primitive_w32: true,
                pixel_w32: true
            }
        );
        // VS_W32_EN (bit 23) alone is not the primitive shader's width.
        let vs_only = [write(0xA2D5, 1 << 23), write(0xA1B6, 0x0000_0002)];
        assert_eq!(wave_widths_at(&vs_only), WaveWidths::default());
        let fixture = [write(0xA2D5, 0x00c1_2010), write(0xA2D5, 0x0200_2000)];
        assert!(!wave_widths_at(&fixture).primitive_w32);
    }

    /// `CB_COLOR0_INFO` decodes to the element layout a written-back frame needs: `0x000180a8` is
    /// `8_8_8_8` `UNORM` in standard order, the same value with `COMP_SWAP` = 1 is B, G, R, A, and
    /// a reversed order or another format is not one a frame is written into.
    #[test]
    fn colour_target_info_decodes_format_number_and_swap() {
        use super::{ComponentSwap, decode_colour_target_format};
        let measured = decode_colour_target_format(0x0001_80a8);
        assert_eq!(
            (measured.format, measured.number_type, measured.swap),
            (10, 0, ComponentSwap::Standard)
        );
        assert!(measured.is_rgba8_class());
        let alternate = decode_colour_target_format(0x0001_80a8 | (1 << 11));
        assert_eq!(alternate.swap, ComponentSwap::Alternate);
        assert!(alternate.is_rgba8_class());
        assert!(!decode_colour_target_format(0x0001_80a8 | (2 << 11)).is_rgba8_class());
        assert!(!decode_colour_target_format(0x0001_80a8 | (1 << 8)).is_rgba8_class());
        assert!(!decode_colour_target_format(0x0001_8000 | (12 << 2)).is_rgba8_class());
    }

    #[test]
    fn every_shader_address_is_a_consecutive_low_high_pair() {
        // The register numbers are transcribed, but their shape is checkable: a shader's start
        // address comes from `SPI_SHADER_PGM_LO/HI`, per stage, as a pair. So each stage has
        // exactly one of each half, with the high immediately above the low. A transposed digit or
        // a dropped half breaks that.
        let table = Vocabulary::builtin().expect("packets");

        let mut by_stage: std::collections::BTreeMap<&str, (Option<u32>, Option<u32>)> =
            std::collections::BTreeMap::new();
        for (register, (stage, is_high)) in &table.shader_registers {
            let slot = by_stage.entry(stage.as_str()).or_default();
            if *is_high {
                slot.1 = Some(*register);
            } else {
                slot.0 = Some(*register);
            }
        }

        assert!(
            !by_stage.is_empty(),
            "the table names no shader addresses at all"
        );
        for (stage, (low, high)) in by_stage {
            let low = low.unwrap_or_else(|| panic!("{stage} has a high half and no low"));
            let high = high.unwrap_or_else(|| panic!("{stage} has a low half and no high"));
            assert_eq!(
                high,
                low + 1,
                "{stage}: the reference forms an address from a consecutive LO/HI pair, so                  {high:#x} should be one above {low:#x}"
            );
        }
    }
    use super::{BlendFactor, CombineFunc, Place, StencilOp};
    use super::{
        BufferDescriptor, ColourTarget, ColourTargetExtent, CompareFunc, DispatchCall, DrawKind,
        DrawOrDispatch, ImageDescriptor, RegisterWrite, Scissor, SwizzleMode, Vocabulary,
        blend_control_at, buffer_descriptor_at, colour_swizzle_mode_at, colour_target_at,
        colour_target_extent_at, correlate_draws, decode_blend_control, decode_blend_factor,
        decode_buffer_descriptor, decode_colour_swizzle_mode, decode_colour_target_extent,
        decode_combine_func, decode_depth_control, decode_image_descriptor, decode_scissor,
        decode_stencil_control, decode_target_mask, depth_control_at, dispatch_calls, draw_calls,
        register_writes, scissor_at, shader_candidates, stencil_control_at, target_mask_at,
    };
    use crate::packet::walk;

    fn stream(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    fn command(opcode: u8, body_dwords: u32) -> u32 {
        (3 << 30) | ((body_dwords - 1) << 16) | (u32::from(opcode) << 8)
    }

    fn register_packet(base: u16, body_dwords: u32) -> u32 {
        ((body_dwords - 1) << 16) | u32::from(base)
    }

    fn vocabulary() -> Vocabulary {
        Vocabulary::load(
            r#"
            [[opcode]]
            value = 0x76
            name = "SET_SH_REG"

            [[register_write]]
            opcode = 0x76
            name = "SET_SH_REG"
            base = 0x2C00

            [[shader_address]]
            register = 0x2C0C
            stage = "fragment"
            half = "low"

            [[shader_address]]
            register = 0x2C0D
            stage = "fragment"
            half = "high"
            "#,
        )
        .expect("vocabulary")
    }

    #[test]
    fn a_type_zero_packet_writes_consecutive_registers_from_its_header() {
        let bytes = stream(&[register_packet(0x30, 3), 0xAAAA, 0xBBBB, 0xCCCC]);
        let writes = register_writes(&walk(&bytes), &bytes, &vocabulary());
        assert_eq!(writes.len(), 3);
        assert_eq!(writes[0].register, 0x30);
        assert_eq!(writes[1].register, 0x31);
        assert_eq!(writes[2].value, 0xCCCC);
    }

    #[test]
    fn a_set_register_packet_takes_its_index_from_the_first_body_word() {
        // The index is data, not header; reading it as a value would shift every register by one.
        let bytes = stream(&[command(0x76, 3), 0x0C, 0x1111, 0x2222]);
        let writes = register_writes(&walk(&bytes), &bytes, &vocabulary());
        assert_eq!(writes.len(), 2, "the index word is not a value");
        assert_eq!(writes[0].register, 0x2C0C, "base plus index");
        assert_eq!(writes[0].value, 0x1111);
        assert_eq!(writes[1].register, 0x2C0D);
    }

    #[test]
    fn a_packet_the_vocabulary_does_not_know_writes_nothing() {
        // No base is guessed for an unknown opcode, so its body is attributed to no register.
        let bytes = stream(&[command(0x2D, 2), 0x00, 0x1111]);
        let writes = register_writes(&walk(&bytes), &bytes, &vocabulary());
        assert!(writes.is_empty());
    }

    #[test]
    fn both_halves_reassemble_into_one_address() {
        let bytes = stream(&[command(0x76, 3), 0x0C, 0x8000_0000, 0x0000_00FF]);
        let walked = walk(&bytes);
        let writes = register_writes(&walked, &bytes, &vocabulary());
        let candidates = shader_candidates(&writes, &vocabulary());
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].stage, "fragment");
        assert_eq!(candidates[0].address, 0x0000_FF80_0000_0000); // the halves, in 256-byte units
    }

    #[test]
    fn a_lone_half_produces_no_candidate() {
        // An address missing its high word reads as an ordinary low address, so it is skipped.
        let bytes = stream(&[command(0x76, 2), 0x0C, 0x8000_0000]);
        let walked = walk(&bytes);
        let writes = register_writes(&walked, &bytes, &vocabulary());
        assert!(shader_candidates(&writes, &vocabulary()).is_empty());
    }

    #[test]
    fn a_later_bind_replaces_an_earlier_one() {
        // A submission rebinds a stage several times; the last write before a draw is the one in
        // force.
        let bytes = stream(&[
            command(0x76, 3),
            0x0C,
            0x1111_1111,
            0x0000_0001,
            command(0x76, 3),
            0x0C,
            0x2222_2222,
            0x0000_0002,
        ]);
        let walked = walk(&bytes);
        let writes = register_writes(&walked, &bytes, &vocabulary());
        let candidates = shader_candidates(&writes, &vocabulary());
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].address, 0x0000_0222_2222_2200);
    }

    /// The viewport transform in force at a draw is its four registers' last writes before it: the
    /// GL context's 1080p transform has a negative y scale, and a draw before the writes, or after
    /// a `VTE_CNTL` disabling the x/y terms, has none.
    #[test]
    fn a_gl_context_s_depth_is_clipped_to_minus_w_and_mapped_by_its_depth_range() {
        use super::{DepthMapping, RegisterWrite, viewport_transform_at};
        let at = |register, value: u32| RegisterWrite {
            packet_offset: 8,
            register,
            value,
        };
        let xy = [
            at(0xA10F, 960f32.to_bits()),
            at(0xA110, 960f32.to_bits()),
            at(0xA111, (-540f32).to_bits()),
            at(0xA112, 540f32.to_bits()),
        ];
        // What the GL context writes for glDepthRange(0, 1): VTE_CNTL 0x43f, ZSCALE and ZOFFSET a
        // half, PA_CL_CLIP_CNTL zero.
        let mut writes = xy.to_vec();
        writes.extend([
            at(0xA206, 0x43F),
            at(0xA113, 0.5f32.to_bits()),
            at(0xA114, 0.5f32.to_bits()),
            at(0xA204, 0),
        ]);
        let depth = |writes: &[RegisterWrite]| {
            viewport_transform_at(writes, 100)
                .expect("the x/y terms were written")
                .depth
        };
        assert_eq!(
            depth(&writes),
            DepthMapping {
                negative_one_to_one: true,
                z_scale: 0.5,
                z_offset: 0.5,
            }
        );
        writes.push(RegisterWrite {
            packet_offset: 9,
            ..at(0xA204, 1 << 19)
        });
        assert!(
            !depth(&writes).negative_one_to_one,
            "DX_CLIP_SPACE_DEF set clips to 0..w"
        );
        writes.push(RegisterWrite {
            packet_offset: 10,
            ..at(0xA206, 0x40F)
        });
        assert_eq!(
            (depth(&writes).z_scale, depth(&writes).z_offset),
            (1.0, 0.0),
            "the z terms off take z unchanged"
        );
        assert_eq!(
            depth(&xy),
            DepthMapping::IDENTITY,
            "a stream that set none of it keeps the identity"
        );
    }

    #[test]
    fn the_viewport_transform_is_read_with_its_sign() {
        use super::{RegisterWrite, viewport_transform_at};
        let at = |register, value: f32, packet_offset| RegisterWrite {
            packet_offset,
            register,
            value: value.to_bits(),
        };
        // gl_compute_vport for a 1920x1080 viewport at the origin: 960, 960, -540, 540.
        let mut writes = vec![
            at(0xA10F, 960.0, 8),
            at(0xA110, 960.0, 8),
            at(0xA111, -540.0, 8),
            at(0xA112, 540.0, 8),
        ];
        let transform = viewport_transform_at(&writes, 100).expect("all four were written");
        assert_eq!(
            (
                transform.x_scale,
                transform.x_offset,
                transform.y_scale,
                transform.y_offset
            ),
            (960.0, 960.0, -540.0, 540.0)
        );
        assert!(
            viewport_transform_at(&writes, 8).is_none(),
            "not before the writes"
        );
        writes.push(RegisterWrite {
            packet_offset: 20,
            register: 0xA206,
            value: 0x430, // VTE_CNTL with the x/y enables clear
        });
        assert!(viewport_transform_at(&writes, 100).is_none());
    }

    /// A draw's positions are clip space unless `PA_CL_VTE_CNTL` turns the x/y terms off; with
    /// every scale and offset off and x, y and z pre-divided - the value radeonsi gives a
    /// window-space shader (`si_state_shaders.cpp:1321-1322`) - they are window space (D731); any
    /// other value with the x/y terms off is named as unmodelled.
    #[test]
    fn a_draw_s_position_space_is_read_from_vte_cntl() {
        use super::{PositionSpace, position_space};
        let with = |value: Option<u32>| position_space(|_| value);
        assert_eq!(with(None), PositionSpace::Clip, "nothing written");
        assert_eq!(
            with(Some(0x43F)),
            PositionSpace::Clip,
            "radeonsi's clip-space value"
        );
        assert_eq!(with(Some(0x300)), PositionSpace::Window);
        assert_eq!(
            with(Some(0x700)),
            PositionSpace::Unmodelled(0x700),
            "W0 not 1/W"
        );
        assert_eq!(
            with(Some(0x310)),
            PositionSpace::Unmodelled(0x310),
            "a z scale"
        );
        assert_eq!(
            with(Some(0x430)),
            PositionSpace::Unmodelled(0x430),
            "not pre-divided"
        );
    }

    /// The vertex program is read from `PGM_LO/HI_ES`, not `PGM_LO/HI_VS`: on this generation the
    /// NGG wave takes its program counter from ES (Mesa `radv_shader.c:2078-2084`). A stream that
    /// points ES at one program and VS at another names the ES one.
    #[test]
    fn the_vertex_program_is_the_es_pair() {
        let vocabulary = Vocabulary::builtin().expect("the built-in vocabulary loads");
        let bytes = stream(&[
            // SET_SH_REG 0x2C48/0x2C49 (VS): one program ...
            command(0x76, 3),
            0x48,
            0x1111_1111,
            0x0000_0001,
            // ... SET_SH_REG 0x2CC8/0x2CC9 (ES): the one the hardware runs.
            command(0x76, 3),
            0xC8,
            0x2222_2222,
            0x0000_0002,
        ]);
        let walked = walk(&bytes);
        let writes = register_writes(&walked, &bytes, &vocabulary);
        let vertex: Vec<_> = shader_candidates(&writes, &vocabulary)
            .into_iter()
            .filter(|c| c.stage == "vertex")
            .collect();
        assert_eq!(vertex.len(), 1);
        assert_eq!(vertex[0].address, 0x0000_0222_2222_2200, "the ES program");
    }

    /// Each draw runs the shader bound before it, not the stream's last: a draw between two binds
    /// of a stage saw the first.
    #[test]
    fn a_draw_between_two_binds_sees_the_first() {
        use super::shader_candidates_before;
        let bytes = stream(&[
            command(0x76, 3),
            0x0C,
            0x1111_1111,
            0x0000_0001,
            command(0x76, 3),
            0x0C,
            0x2222_2222,
            0x0000_0002,
        ]);
        let walked = walk(&bytes);
        let writes = register_writes(&walked, &bytes, &vocabulary());
        let second_bind = walked.packets[1].offset;
        let between = shader_candidates_before(&writes, &vocabulary(), second_bind);
        assert_eq!(between.len(), 1);
        assert_eq!(between[0].address, 0x0000_0111_1111_1100, "the first bind");
        let after = shader_candidates_before(&writes, &vocabulary(), u32::MAX);
        assert_eq!(after[0].address, 0x0000_0222_2222_2200, "the second bind");
        assert!(shader_candidates_before(&writes, &vocabulary(), 0).is_empty());
    }

    #[test]
    fn each_draw_is_correlated_with_the_shader_live_when_it_issued() {
        // Bind fragment A, draw; rebind fragment B, draw. Each draw carries its own bind.
        let bytes = stream(&[
            command(0x76, 3),
            0x0C,
            0x0000_00A0,
            0x0000_0000, // SET_SH_REG fragment low=0xA0 high=0
            command(0x2D, 2),
            3,
            0, // DRAW_INDEX_AUTO, 3 vertices
            command(0x76, 3),
            0x0C,
            0x0000_00B0,
            0x0000_0000, // rebind fragment low=0xB0
            command(0x2D, 2),
            6,
            0, // DRAW_INDEX_AUTO, 6 vertices
        ]);
        let correlations = correlate_draws(&walk(&bytes), &bytes, &vocabulary());
        assert_eq!(correlations.len(), 2, "two draws");

        let DrawOrDispatch::Draw(first) = correlations[0].work else {
            panic!("first is a draw");
        };
        assert_eq!(first.kind, DrawKind::Auto { vertices: 3 });
        assert_eq!(correlations[0].shaders.len(), 1);
        assert_eq!(correlations[0].shaders[0].stage, "fragment");
        assert_eq!(correlations[0].shaders[0].address, 0xA000, "the first bind");

        let DrawOrDispatch::Draw(second) = correlations[1].work else {
            panic!("second is a draw");
        };
        assert_eq!(second.kind, DrawKind::Auto { vertices: 6 });
        assert_eq!(
            correlations[1].shaders[0].address, 0xB000,
            "the rebind, not the first"
        );
    }

    #[test]
    fn a_draw_before_any_shader_bind_correlates_to_no_shader() {
        // A bind after the draw is not live at it.
        let bytes = stream(&[
            command(0x2D, 2),
            3,
            0, // draw with nothing bound yet
            command(0x76, 3),
            0x0C,
            0x0000_00A0,
            0x0000_0000, // bind AFTER the draw
        ]);
        let correlations = correlate_draws(&walk(&bytes), &bytes, &vocabulary());
        assert_eq!(correlations.len(), 1);
        assert!(
            correlations[0].shaders.is_empty(),
            "a later bind is not live at an earlier draw"
        );
    }

    #[test]
    fn a_dispatch_and_a_draw_are_correlated_in_stream_order() {
        // Both kinds of work are correlated, ordered by their position, each carrying the shader
        // bound before either of them.
        let bytes = stream(&[
            command(0x76, 3),
            0x0C,
            0x0000_00A0,
            0x0000_0000,
            command(0x15, 4),
            2,
            3,
            4,
            0, // DISPATCH_DIRECT groups 2,3,4
            command(0x2D, 2),
            3,
            0, // draw
        ]);
        let correlations = correlate_draws(&walk(&bytes), &bytes, &vocabulary());
        assert_eq!(correlations.len(), 2);
        assert!(
            matches!(correlations[0].work, DrawOrDispatch::Dispatch(_)),
            "the dispatch sits first in the stream"
        );
        assert!(matches!(correlations[1].work, DrawOrDispatch::Draw(_)));
        assert_eq!(correlations[0].shaders[0].address, 0xA000);
        assert_eq!(correlations[1].shaders[0].address, 0xA000);
    }

    #[test]
    fn a_capture_read_from_a_file_walks_into_per_draw_shaders() {
        // End to end: a dword stream on disk, read back, walked, and reported as a draw carrying
        // the shader address live when it issued.
        let bytes = stream(&[
            command(0x76, 3),
            0x0C,
            0x0000_00A0,
            0x0000_0000,
            command(0x2D, 2),
            3,
            0,
        ]);
        let path =
            std::env::temp_dir().join(format!("orbistoun-capture-{}.bin", std::process::id()));
        std::fs::write(&path, &bytes).expect("write the synthetic capture");
        let read = std::fs::read(&path).expect("read the capture back");
        std::fs::remove_file(&path).ok();

        let correlations = correlate_draws(&walk(&read), &read, &vocabulary());
        assert_eq!(correlations.len(), 1);
        assert_eq!(correlations[0].shaders[0].stage, "fragment");
        assert_eq!(correlations[0].shaders[0].address, 0xA000);
    }

    #[test]
    fn a_half_that_is_neither_low_nor_high_is_refused() {
        // Defaulting an unrecognised value to "high" would swap the halves of every address it
        // touched.
        let result = Vocabulary::load(
            r#"
            [[shader_address]]
            register = 0x2C0C
            stage = "fragment"
            half = "middle"
            "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn the_builtin_vocabulary_loads_and_names_things() {
        let vocabulary = Vocabulary::builtin().expect("builtin");
        assert_eq!(vocabulary.opcode_name(0x2D), Some("DRAW_INDEX_AUTO"));
        assert!(vocabulary.register_base(0x76).is_some());
        assert!(vocabulary.shader_register_count() >= 2);
        // An unnamed opcode reports as unnamed, so a gap in the vocabulary stays visible.
        assert_eq!(vocabulary.opcode_name(0xFE), None);
    }

    /// An auto draw carries its vertex count, and takes the instance count set before it.
    #[test]
    fn an_auto_draw_reads_its_vertices_and_the_running_instance_count() {
        // NUM_INSTANCES then DRAW_INDEX_AUTO, the shape the captured GL cube uses.
        let bytes = stream(&[command(0x2F, 1), 2, command(0x2D, 2), 3, 0]);
        let draws = draw_calls(&walk(&bytes), &bytes);
        assert_eq!(draws.len(), 1);
        assert_eq!(draws[0].instances, 2);
        assert_eq!(draws[0].kind, DrawKind::Auto { vertices: 3 });
    }

    /// An indexed draw is read from its own body, address and count both: the measured
    /// `DRAW_INDEX_2` body is `[max_size, addr_lo, addr_hi, index_count, initiator]`.
    #[test]
    fn an_indexed_draw_is_decoded_from_its_own_measured_body() {
        let address: u64 = 0x1_2345_6780;
        let lo = u32::try_from(address & 0xffff_ffff).unwrap();
        let hi = u32::try_from(address >> 32).unwrap();
        let bytes = stream(&[
            command(0x2F, 1),
            4, // four instances
            command(0x27, 5),
            0,  // max_size
            lo, // index-buffer address, low half first
            hi,
            36, // index count
            0,  // initiator
        ]);
        let draws = draw_calls(&walk(&bytes), &bytes);
        assert_eq!(draws.len(), 1, "the indexed draw is a draw, not nothing");
        assert_eq!(draws[0].instances, 4);
        assert_eq!(
            draws[0].kind,
            DrawKind::Indexed {
                indices: 36,
                address,
            }
        );
    }

    /// A truncated indexed draw is dropped, not read past its body: reading the missing dword would
    /// fabricate an index count from whatever followed the packet.
    #[test]
    fn a_short_indexed_draw_is_dropped_rather_than_read_past() {
        // Header claims two body words, far short of the five DRAW_INDEX_2 needs.
        let bytes = stream(&[command(0x27, 2), 0, 0]);
        let draws = draw_calls(&walk(&bytes), &bytes);
        assert!(
            draws.is_empty(),
            "a body too short to hold a count is no draw"
        );
    }

    /// A buffer descriptor's fields land in the bits the reference gives them: base across dword 0
    /// and the low half of dword 1, stride in the next fourteen bits, record count in dword 2.
    #[test]
    fn a_buffer_descriptor_decodes_its_base_stride_and_records() {
        // base 0x1_2345_6780, stride 16, records 4.
        let base: u64 = 0x1_2345_6780;
        let word0 = (base & 0xFFFF_FFFF) as u32;
        let word1 = ((base >> 32) as u32 & 0xFFFF) | (16 << 16);
        let descriptor = decode_buffer_descriptor([word0, word1, 4, 0]);
        assert_eq!(
            descriptor,
            BufferDescriptor {
                base,
                stride: 16,
                records: 4,
                unsupported: false,
            }
        );
        // Four records of sixteen bytes is a sixty-four byte buffer.
        assert_eq!(descriptor.byte_len(), 64);
    }

    /// A raw buffer's record count is a byte count, and unmodelled addressing is refused.
    #[test]
    fn a_raw_buffer_is_bytes_and_swizzling_is_unsupported() {
        // Stride zero: records is a byte length.
        let raw = decode_buffer_descriptor([0x1000, 0, 256, 0]);
        assert_eq!(raw.stride, 0);
        assert_eq!(raw.byte_len(), 256);
        assert!(!raw.unsupported);

        // Swizzle-enable (bit 63 = dword 1 bit 31) and add-thread-id (bit 119 = dword 3 bit 23)
        // are addressing this does not model.
        assert!(decode_buffer_descriptor([0, 1 << 31, 4, 0]).unsupported);
        assert!(decode_buffer_descriptor([0, 0, 4, 1 << 23]).unsupported);
    }

    /// A descriptor is read from the live values of four consecutive registers: the most recent
    /// write to each wins, and a descriptor missing any register is not one to bind.
    #[test]
    fn a_descriptor_reads_the_live_register_values() {
        let write = |register, value| RegisterWrite {
            packet_offset: 0,
            register,
            value,
        };
        let writes = vec![
            write(8, 0xDEAD_BEEF), // a stale value...
            write(8, 0x0000_1000), // ...overwritten before the descriptor is read
            write(9, 16 << 16),
            write(10, 4),
            write(11, 0),
        ];
        let descriptor = buffer_descriptor_at(&writes, 8).expect("all four registers set");
        assert_eq!(descriptor.base, 0x1000);
        assert_eq!(descriptor.stride, 16);
        assert_eq!(descriptor.records, 4);

        // One register short: no descriptor.
        assert!(
            buffer_descriptor_at(&writes, 9).is_none(),
            "register 12 was never written"
        );
    }

    /// A direct compute dispatch decodes its three workgroup counts, and a body too short for them
    /// is no dispatch.
    #[test]
    fn a_direct_dispatch_decodes_its_workgroup_counts() {
        // DISPATCH_DIRECT (0x15) with four body words: 8, 4, 1, and an initiator flag.
        let bytes = stream(&[command(0x15, 4), 8, 4, 1, 1]);
        let dispatches = dispatch_calls(&walk(&bytes), &bytes);
        assert_eq!(dispatches.len(), 1);
        assert_eq!(
            dispatches[0],
            DispatchCall {
                packet_offset: 0,
                groups: [8, 4, 1],
            }
        );

        // A body of two words cannot hold three counts.
        let short = stream(&[command(0x15, 2), 8, 4]);
        assert!(dispatch_calls(&walk(&short), &short).is_empty());
    }

    /// A colour target decodes its width and height from CB_COLOR0_ATTRIB2.
    ///
    /// The GL cube value `0x01dfc437` is `1920 x 1080`: width minus one in bits 27:14, height minus
    /// one in bits 13:0. Dropping the minus-one, swapping the fields or masking the wrong width all
    /// fail.
    #[test]
    fn a_colour_target_decodes_its_width_and_height() {
        assert_eq!(
            decode_colour_target_extent(0x01df_c437),
            ColourTargetExtent {
                width: 1920,
                height: 1080,
            }
        );

        // Zero in both fields is a one-by-one target: the stored value is one below the pixel
        // count, so a decode that forgot the minus-one would call this zero-by-zero.
        assert_eq!(
            decode_colour_target_extent(0),
            ColourTargetExtent {
                width: 1,
                height: 1,
            }
        );

        // The fields do not bleed into each other: a maximal width leaves the height at its
        // minimum.
        assert_eq!(
            decode_colour_target_extent(0x3FFF << 14),
            ColourTargetExtent {
                width: 16384,
                height: 1,
            }
        );
    }

    /// The extent is the live CB_COLOR0_ATTRIB2 write, and absent when the stream never sets it
    /// (D010).
    #[test]
    fn the_extent_reads_the_live_register_and_is_absent_when_unset() {
        let write = |register, value| RegisterWrite {
            packet_offset: 0,
            register,
            value,
        };
        let writes = vec![
            write(0xA3B0, 0x0000_0000), // a stale one-by-one...
            write(0xA3B0, 0x01df_c437), // ...resized before the draw
        ];
        assert_eq!(
            colour_target_extent_at(&writes),
            Some(ColourTargetExtent {
                width: 1920,
                height: 1080,
            })
        );

        // Only an unrelated register was written: nothing sized the target.
        assert!(
            colour_target_extent_at(&[write(0xA000, 0x01df_c437)]).is_none(),
            "CB_COLOR0_ATTRIB2 was never written"
        );
    }

    #[test]
    fn the_colour_target_pairs_its_base_and_extent_and_refuses_without_either() {
        let write = |register, value| RegisterWrite {
            packet_offset: 0,
            register,
            value,
        };
        // CB_COLOR0_BASE (0xA318) in 256-byte units; CB_COLOR0_ATTRIB2 (0xA3B0) sizing 64x64.
        let writes = vec![
            write(0xA318, 0x0200_0e00), // base -> 0x2000e0000 once shifted
            write(0xA3B0, 0x000f_c03f), // (63<<14)|63 -> 64x64
        ];
        assert_eq!(
            colour_target_at(&writes),
            Some(ColourTarget {
                base: 0x2_000e_0000,
                width: 64,
                height: 64,
                pipe_bank_xor: 0,
                layout: crate::tiling::SurfaceLayout::Rx64Kb,
                place: Place::Whole,
            })
        );
        // A base with no extent cannot be sized; an extent with no base cannot be placed. Both
        // refuse rather than pair a real half with a guessed one.
        assert!(
            colour_target_at(&[write(0xA318, 0x0200_0e00)]).is_none(),
            "no extent"
        );
        assert!(
            colour_target_at(&[write(0xA3B0, 0x000f_c03f)]).is_none(),
            "no base"
        );
    }

    /// A `64KB_R_X` target's base register carries its pipe-bank XOR in the low byte
    /// (`ac_descriptors.c:1477-1481`): Craft's third texture writes `0x040286c0`, whose surface
    /// starts at the 64 KiB block `0x4_0286_0000` with XOR `0xc0`. A linear target's low byte is
    /// address.
    #[test]
    fn a_tiled_target_s_base_splits_into_its_block_and_its_pipe_bank_xor() {
        let write = |register, value| RegisterWrite {
            packet_offset: 0,
            register,
            value,
        };
        let tiled = [
            write(0xA318, 0x0402_86c0),
            write(0xA3B0, 0x003f_c0ff),
            write(0xA3B8, 0x4dc6_c000),
        ];
        assert_eq!(
            colour_target_at(&tiled),
            Some(ColourTarget {
                base: 0x4_0286_0000,
                width: 256,
                height: 256,
                pipe_bank_xor: 0xc0,
                layout: crate::tiling::SurfaceLayout::Rx64Kb,
                place: Place::Whole,
            })
        );
        let linear = [
            write(0xA318, 0x0402_86c0),
            write(0xA3B0, 0x003f_c0ff),
            write(0xA3B8, 0),
        ];
        assert_eq!(
            colour_target_at(&linear).map(|t| (t.base, t.pipe_bank_xor)),
            Some((0x4_0286_c000, 0))
        );
    }

    /// Craft's fourth texture target, as its stream writes it: DCC enabled, pipe-aligned keys at
    /// `0x4_028a_0000`, one level of one 2D slice. Without `DCC_ENABLE` there is none; a mip chain
    /// counts its levels, and a view of other slices is not single-slice.
    #[test]
    fn a_dcc_target_decodes_its_keys_and_its_shape() {
        use super::{ColourTargetDcc, colour_target_dcc_at};
        let write = |register, value| RegisterWrite {
            packet_offset: 0,
            register,
            value,
        };
        let craft = vec![
            write(0xA318, 0x0402_86c0),
            write(0xA31B, 0),
            write(0xA31C, 0x1002_8028),
            write(0xA31D, 0),
            write(0xA325, 0x0402_8a00),
            write(0xA3A8, 0),
            write(0xA3B0, 0x003f_c0ff),
            write(0xA3B8, 0x4dc6_c000),
        ];
        assert_eq!(
            colour_target_dcc_at(&craft),
            Some(ColourTargetDcc {
                dcc: crate::dcc::Dcc {
                    base: 0x4_028a_0000,
                    pipe_aligned: true,
                },
                surface: 0x4_0286_0000,
                extent: ColourTargetExtent {
                    width: 256,
                    height: 256,
                },
                levels: 1,
                single_slice: true,
                single_sample: true,
            })
        );
        let mut plain = craft.clone();
        plain.push(write(0xA31C, 0x0002_8028));
        assert_eq!(colour_target_dcc_at(&plain), None);
        let mut chain = craft.clone();
        chain.push(write(0xA3B0, 0x303f_c0ff));
        let chained = colour_target_dcc_at(&chain).expect("dcc");
        assert_eq!((chained.levels, chained.single_slice), (4, true));
        let mut level = chain;
        level.push(write(0xA31B, 1 << 26));
        assert!(colour_target_dcc_at(&level).expect("dcc").single_slice);
        let mut slices = craft;
        slices.push(write(0xA31B, 1 << 13));
        assert!(!colour_target_dcc_at(&slices).expect("dcc").single_slice);
    }

    /// SuperTuxKart's 2048x1024 target, as its stream writes it: twelve levels (`MAX_MIP` 11) in
    /// `64KB_R_X`, the view on level 0. The target is level 0 where addrlib places it in the
    /// chain, last; a view of level 1 is that level, at its own extent; a level in the tail is the
    /// tail's block, the chain's first, with the level's origin in it; a single level stays at the
    /// base.
    #[test]
    fn a_mip_chains_target_is_the_level_its_view_names() {
        use super::{RegisterWrite, colour_target_at};
        let write = |register, value| RegisterWrite {
            packet_offset: 0,
            register,
            value,
        };
        let chain = vec![
            write(0xA318, 0x0404_c0c0),
            write(0xA31B, 0),
            write(0xA3B0, 0xb1ff_c3ff),
            write(0xA3B8, 0x4dc6_c000),
        ];
        let target = colour_target_at(&chain).expect("a target");
        assert_eq!(
            (
                target.base,
                target.width,
                target.height,
                target.pipe_bank_xor
            ),
            (0x4_04c0_0000 + 2_883_584, 2048, 1024, 0xc0)
        );
        let mut one = chain.clone();
        one.push(write(0xA31B, 1 << 26));
        assert_eq!(
            colour_target_extent_at(&one),
            Some(ColourTargetExtent {
                width: 1024,
                height: 512
            }),
            "the attachment is the level's"
        );
        let target = colour_target_at(&one).expect("level 1");
        assert_eq!(
            (target.base, target.width, target.height),
            (0x4_04c0_0000 + 786_432, 1024, 512)
        );
        let mut tail = chain.clone();
        tail.push(write(0xA31B, 5 << 26));
        let target = colour_target_at(&tail).expect("level 5");
        assert_eq!(
            (target.base, target.width, target.height, target.place),
            (
                0x4_04c0_0000,
                64,
                32,
                Place::Within {
                    span: (128, 128),
                    origin: (64, 0)
                }
            )
        );
        let mut single = chain;
        single.push(write(0xA3B0, 0x01ff_c3ff));
        assert_eq!(
            colour_target_at(&single).map(|t| t.base),
            Some(0x4_04c0_0000)
        );
    }

    #[test]
    fn the_colour_target_takes_the_most_recent_base() {
        // A submission that rebinds the target before drawing: the last base wins, like the extent.
        let write = |register, value| RegisterWrite {
            packet_offset: 0,
            register,
            value,
        };
        let writes = vec![
            write(0xA318, 0x0100_0000), // a stale base...
            write(0xA3B0, 0x000f_c03f),
            write(0xA318, 0x0200_0e00), // ...rebound before the draw
        ];
        assert_eq!(
            colour_target_at(&writes).expect("both set").base,
            0x2_000e_0000
        );
    }

    #[test]
    fn the_target_mask_decodes_per_target_channels() {
        // MRT0 writes all four channels (0xF); MRT1 writes red+alpha (0x9); the rest none.
        let mask = decode_target_mask(0x0000_009F);
        assert_eq!(mask.targets[0], 0xF, "MRT0 RGBA");
        assert_eq!(mask.targets[1], 0x9, "MRT1 red+alpha");
        assert_eq!(mask.targets[2], 0x0);
        assert!(mask.writes_target(0));
        assert!(mask.writes_target(1));
        assert!(!mask.writes_target(2), "MRT2 is disabled");
        assert_eq!(mask.active_targets(), 2);
    }

    /// The bases counted are the ones draws drew into, and the one left in force: a base
    /// overwritten before any draw drew into it is not a target, and a stream whose draws share
    /// one base and leave it in force has one. A different base in force after the last draw is a
    /// second, since what is read back is the base left in force.
    #[test]
    fn colour_target_bases_are_the_ones_draws_drew_into_and_the_one_left() {
        use super::{CB_COLOR0_BASE, RegisterWrite, colour_target_bases_in};
        let base = |packet_offset, value| RegisterWrite {
            packet_offset,
            register: CB_COLOR0_BASE,
            value,
        };
        let writes = [base(0, 0x100), base(8, 0x200), base(40, 0x200)];
        assert_eq!(colour_target_bases_in(&writes, &[16, 32]), 1);
        assert_eq!(
            colour_target_bases_in(&writes, &[4, 32]),
            2,
            "the first draw drew into 0x100"
        );
        let after = [base(8, 0x200), base(40, 0x300)];
        assert_eq!(colour_target_bases_in(&after, &[16]), 2);
        assert_eq!(
            colour_target_bases_in(&after, &[]),
            1,
            "no draw: the one left"
        );
        assert_eq!(colour_target_bases_in(&[], &[16]), 0);
    }

    #[test]
    fn the_target_mask_reads_the_live_register_and_is_absent_when_unset() {
        let write = |register, value| RegisterWrite {
            packet_offset: 0,
            register,
            value,
        };
        // The last write to CB_TARGET_MASK (0xA08E) wins, like the other target state.
        let writes = vec![
            write(0xA08E, 0x0000_0000), // a stale all-off...
            write(0xA08E, 0x0000_000F), // ...one target, all channels, before the draw
        ];
        assert_eq!(target_mask_at(&writes).expect("set").active_targets(), 1);
        assert!(
            target_mask_at(&[write(0xA000, 0x0000_000F)]).is_none(),
            "CB_TARGET_MASK was never written"
        );
    }

    #[test]
    fn the_depth_control_decodes_its_test_state() {
        // Z on + write, ZFUNC LEQUAL(3); stencil on, front ALWAYS(7), back NOTEQUAL(5) with
        // BACKFACE_ENABLE; depth-bounds off. STENCIL_ENABLE bit 0, Z_ENABLE 1, Z_WRITE 2, ZFUNC
        // 4:6, BACKFACE 7, STENCILFUNC 8:10, STENCILFUNC_BF 20:22.
        let dc = decode_depth_control(0x0050_07B7);
        assert!(dc.depth_test_enable);
        assert!(dc.depth_write_enable);
        assert_eq!(dc.depth_func, CompareFunc::LessEqual);
        assert!(!dc.depth_bounds_test_enable, "bit 3 was not set");
        assert!(dc.stencil_test_enable);
        assert_eq!(dc.stencil_func, CompareFunc::Always);
        assert!(dc.backface_enable);
        assert_eq!(dc.stencil_func_backface, CompareFunc::NotEqual);

        // All-zero: every test off, every compare NEVER - decoded rather than assumed.
        let off = decode_depth_control(0);
        assert!(!off.depth_test_enable && !off.stencil_test_enable && !off.depth_write_enable);
        assert_eq!(off.depth_func, CompareFunc::Never);
    }

    #[test]
    fn the_depth_control_reads_the_live_register_and_is_absent_when_unset() {
        let write = |register, value| RegisterWrite {
            packet_offset: 0,
            register,
            value,
        };
        // The last write to DB_DEPTH_CONTROL (0xA200) wins.
        let writes = vec![
            write(0xA200, 0x0000_0000),          // depth off...
            write(0xA200, 0x0000_0002 | 0x0010), // ...then Z_ENABLE with ZFUNC LESS(1) before the draw
        ];
        let dc = depth_control_at(&writes).expect("set");
        assert!(dc.depth_test_enable);
        assert_eq!(dc.depth_func, CompareFunc::Less);
        assert!(
            depth_control_at(&[write(0xA000, 0x2)]).is_none(),
            "DB_DEPTH_CONTROL was never written"
        );
    }

    #[test]
    fn the_stencil_control_decodes_its_six_operations() {
        // A distinct op in each field, so a misplaced one fails. STENCILFAIL 0:3, STENCILZPASS 4:7,
        // STENCILZFAIL 8:11, then the three back-face fields 12:15, 16:19, 20:23.
        let sc = decode_stencil_control(0x00F5_1730);
        assert_eq!(sc.fail_op, StencilOp::Keep, "STENCILFAIL = 0");
        assert_eq!(sc.depth_pass_op, StencilOp::ReplaceTest, "STENCILZPASS = 3");
        assert_eq!(sc.depth_fail_op, StencilOp::Invert, "STENCILZFAIL = 7");
        assert_eq!(sc.back_fail_op, StencilOp::Zero, "STENCILFAIL_BF = 1");
        assert_eq!(
            sc.back_depth_pass_op,
            StencilOp::AddClamp,
            "STENCILZPASS_BF = 5"
        );
        assert_eq!(
            sc.back_depth_fail_op,
            StencilOp::Xnor,
            "STENCILZFAIL_BF = 15"
        );
    }

    #[test]
    fn the_stencil_control_reads_the_live_register_and_is_absent_when_unset() {
        let write = |register, value| RegisterWrite {
            packet_offset: 0,
            register,
            value,
        };
        // The last write to DB_STENCIL_CONTROL (0xA10B) wins.
        let writes = vec![
            write(0xA10B, 0x0000_0000), // all keep...
            write(0xA10B, 0x0000_0001), // ...then STENCILFAIL = ZERO(1) before the draw
        ];
        let sc = stencil_control_at(&writes).expect("set");
        assert_eq!(sc.fail_op, StencilOp::Zero);
        assert_eq!(sc.depth_pass_op, StencilOp::Keep, "the rest stay at 0");
        assert!(
            stencil_control_at(&[write(0xA000, 0x1)]).is_none(),
            "DB_STENCIL_CONTROL was never written"
        );
    }

    #[test]
    fn the_blend_control_decodes_its_factors_and_functions() {
        // A distinct factor/function in each field so a misplaced one fails: colour SRC_ALPHA(4),
        // MAX(3), ONE_MINUS_SRC_ALPHA(5); alpha ONE(1), DST_MINUS_SRC(4), ZERO(0); separate-alpha
        // and enable on, ROP3 not disabled.
        let bc = decode_blend_control(0x6081_0564);
        assert_eq!(bc.color_src, BlendFactor::SrcAlpha);
        assert_eq!(bc.color_combine, CombineFunc::MaxDstSrc);
        assert_eq!(bc.color_dst, BlendFactor::OneMinusSrcAlpha);
        assert_eq!(bc.alpha_src, BlendFactor::One);
        assert_eq!(bc.alpha_combine, CombineFunc::DstMinusSrc);
        assert_eq!(bc.alpha_dst, BlendFactor::Zero);
        assert!(bc.separate_alpha_blend);
        assert!(bc.enable);
        assert!(!bc.disable_rop3, "bit 31 was not set");

        // Reserved encodings are kept raw, not mapped to a defined factor or function.
        assert_eq!(decode_blend_factor(31), BlendFactor::Other(31));
        assert_eq!(decode_combine_func(7), CombineFunc::Other(7));
    }

    #[test]
    fn the_blend_control_reads_the_live_register_and_is_absent_when_unset() {
        let write = |register, value| RegisterWrite {
            packet_offset: 0,
            register,
            value,
        };
        // The last write to CB_BLEND0_CONTROL (0xA1E0) wins.
        let writes = vec![
            write(0xA1E0, 0x0000_0000), // blend off...
            write(0xA1E0, 0x4000_0000), // ...then ENABLE (bit 30) before the draw
        ];
        let bc = blend_control_at(&writes).expect("set");
        assert!(bc.enable);
        assert!(!bc.separate_alpha_blend, "bit 29 was not set");
        assert!(
            blend_control_at(&[write(0xA000, 0x4000_0000)]).is_none(),
            "CB_BLEND0_CONTROL was never written"
        );
    }

    #[test]
    fn the_swizzle_mode_decodes_the_modes_orbistoun_handles() {
        // The 64x64 point-draw capture's ATTRIB3 value decodes to 64KB_R_X.
        assert_eq!(
            decode_colour_swizzle_mode(0x08c6_c000),
            SwizzleMode::Tiled64KbRX,
            "the measured -a1f7 tiling"
        );
        // COLOR_SW_MODE is bits 18:14, so field value 0 is linear whatever the other bits carry.
        assert_eq!(decode_colour_swizzle_mode(0x0000_0000), SwizzleMode::Linear);
        // An unmodelled mode is carried by its raw value: 4KB_S is ADDR_SW_4KB_S = 5, so `5 << 14`
        // = 0x0001_4000 in the field.
        assert_eq!(
            decode_colour_swizzle_mode(0x0001_4000),
            SwizzleMode::Other(5)
        );
    }

    #[test]
    fn the_swizzle_mode_reads_the_live_register_and_is_absent_when_unset() {
        let write = |register, value| RegisterWrite {
            packet_offset: 0,
            register,
            value,
        };
        assert_eq!(
            colour_swizzle_mode_at(&[write(0xA3B8, 0x08c6_c000)]),
            Some(SwizzleMode::Tiled64KbRX)
        );
        assert!(
            colour_swizzle_mode_at(&[write(0xA000, 0x08c6_c000)]).is_none(),
            "CB_COLOR0_ATTRIB3 was never written"
        );
    }

    /// An image descriptor decodes its base, dimensions and format from the GFX10 layout.
    ///
    /// Built as the open-source driver constructs one (`S_00A004_WIDTH_LO(w - 1)` against
    /// `S_00A008_WIDTH_HI((w - 1) >> 2)`, `S_00A008_HEIGHT(h - 1)`). At 1920 x 1080 the width's low
    /// two bits sit in dword 1 and the rest in dword 2; the small case carries a base with a high
    /// byte, so both halves of the address are exercised.
    #[test]
    fn an_image_descriptor_decodes_its_base_dimensions_format_and_tiling() {
        // Encode the dwords that carry base, format, extent and tiling, as ac_descriptors.c does.
        let encode = |base: u64, width: u32, height: u32, format: u32, sw_mode: u32| -> [u32; 8] {
            let (w1, h1) = (width - 1, height - 1);
            let word0 = u32::try_from((base >> 8) & 0xFFFF_FFFF).expect("32 bits");
            let base_hi = u32::try_from((base >> 40) & 0xFF).expect("8 bits");
            let word1 = base_hi | (format << 20) | ((w1 & 0x3) << 30);
            let word2 = (w1 >> 2) | ((h1 & 0x3FFF) << 14);
            let word3 = (sw_mode & 0x1F) << 20; // SQ_IMG_RSRC_WORD3.SW_MODE at bits 24:20
            [word0, word1, word2, word3, 0, 0, 0, 0]
        };

        // A 2x2 linear texture at a base whose high byte is set, format code 0x0A.
        assert_eq!(
            decode_image_descriptor(encode(0x1234_5670_0000, 2, 2, 0x0A, 0)),
            ImageDescriptor {
                base: 0x1234_5670_0000,
                width: 2,
                height: 2,
                format: 0x0A,
                tiling: SwizzleMode::Linear,
                pipe_bank_xor: 0,
                levels: 1,
                base_level: 0,
                last_level: 0,
                compression: None,
            }
        );

        // 1920 x 1080, tiled 64KB_R_X (ADDR_SW_64KB_R_X = 27): the width split and the tiling read
        // from dword 3.
        assert_eq!(
            decode_image_descriptor(encode(0x2_0000_0000, 1920, 1080, 0x0C, 27)),
            ImageDescriptor {
                base: 0x2_0000_0000,
                width: 1920,
                height: 1080,
                format: 0x0C,
                tiling: SwizzleMode::Tiled64KbRX,
                pipe_bank_xor: 0,
                levels: 1,
                base_level: 0,
                last_level: 0,
                compression: None,
            }
        );
    }

    /// The texture SuperTuxKart's image copy reads, as its user data holds it: the pipe-bank XOR
    /// split off the base, ten levels (`MAX_MIP` 9) viewed from level 0, and DCC keys at
    /// `0x4_0916_0000`, pipe-aligned. Level 0 is where addrlib places it in that chain, last, at
    /// `0x60000`; a view of level 6 is in the mip tail, at the chain's first block.
    #[test]
    fn a_compressed_mip_chain_texture_decodes_where_its_level_lies() {
        use super::{ImageDescriptor, decode_image_descriptor};
        let words = [
            0x0409_00c0,
            0xc380_0000,
            0x807f_c07f,
            0x91b0_0fac,
            0,
            0x0040_0090,
            0x006b_0000,
            0x0004_0916,
        ];
        let descriptor = decode_image_descriptor(words);
        assert_eq!(
            descriptor,
            ImageDescriptor {
                base: 0x4_0900_0000,
                width: 512,
                height: 512,
                format: 0x38,
                tiling: SwizzleMode::Tiled64KbRX,
                pipe_bank_xor: 0xc0,
                levels: 10,
                base_level: 0,
                last_level: 0,
                compression: Some(crate::dcc::Dcc {
                    base: 0x4_0916_0000,
                    pipe_aligned: true,
                }),
            }
        );
        let level_zero = descriptor.base_level_surface().expect("placed");
        assert_eq!(
            (
                level_zero.base,
                level_zero.width,
                level_zero.pipe_bank_xor,
                level_zero.place
            ),
            (0x4_0906_0000, 512, 0xc0, Place::Whole)
        );
        let tail = ImageDescriptor {
            base_level: 6,
            ..descriptor
        }
        .base_level_surface()
        .expect("placed");
        assert_eq!(
            (
                tail.base,
                tail.width,
                tail.height,
                matches!(tail.place, Place::Within { .. })
            ),
            (0x4_0900_0000, 8, 8, true)
        );
        assert_eq!(
            crate::dcc::chain_meta_bytes(512, 512, 10, true),
            16384,
            "addrlib's dccRamSize for the chain"
        );
    }

    /// A linear image's pitch: with no custom pitch, the width rounded up to 64 texels, as addrlib
    /// lays out `ADDR_SW_LINEAR` under the console's configuration (100 -> 128, 65 -> 128, 64 -> 64,
    /// 1000 -> 1024); with one, the field plus one.
    #[test]
    fn a_linear_pitch_is_256_byte_rows_unless_the_descriptor_names_one() {
        use super::linear_pitch;
        for (width, pitch) in [(100, 128), (65, 128), (64, 64), (512, 512), (1000, 1024)] {
            assert_eq!(linear_pitch(0, width, 4), pitch, "{width}");
        }
        assert_eq!(linear_pitch(0, 100, 1), 256, "256 one-byte texels to a row");
        assert_eq!(linear_pitch(0, 512, 1), 512);
        assert_eq!(linear_pitch(199, 100, 4), 200, "a custom pitch");
        assert_eq!(linear_pitch(1 << 13 | 7, 100, 4), 8200, "PITCH_MSB");
    }

    /// A scissor decodes its rectangle from the GENERIC_SCISSOR corners.
    ///
    /// The draw oracle writes top-left `0x80000000` (origin plus the `WINDOW_OFFSET_DISABLE` flag
    /// at bit 31) and bottom-right `0x00400040` for a 64x64 frame. A sub-rect with a non-zero
    /// origin exercises the top-left, which the full-frame case cannot.
    #[test]
    fn a_scissor_decodes_its_rectangle() {
        assert_eq!(
            decode_scissor(0x8000_0000, 0x0040_0040),
            Scissor {
                x: 0,
                y: 0,
                width: 64,
                height: 64,
            }
        );

        // A sub-rect: top-left (x 16, y 8) is 0x0008_0010, bottom-right (x 48, y 56) is
        // 0x0038_0030.
        let top_left = 0x0008_0010;
        let bottom_right = 0x0038_0030;
        assert_eq!(
            decode_scissor(top_left, bottom_right),
            Scissor {
                x: 16,
                y: 8,
                width: 32,
                height: 48,
            }
        );

        // An inverted rectangle (corners swapped) is empty, not an underflow.
        assert_eq!(decode_scissor(bottom_right, top_left).width, 0);
    }

    /// The scissor reads the live GENERIC_SCISSOR writes, and is absent when a corner is missing.
    #[test]
    fn the_scissor_reads_the_live_registers() {
        let write = |register, value| RegisterWrite {
            packet_offset: 0,
            register,
            value,
        };
        let writes = vec![
            write(0xA090, 0xDEAD_BEEF), // a stale top-left...
            write(0xA090, 0x8000_0000), // ...overwritten before the draw
            write(0xA091, 0x0040_0040),
        ];
        assert_eq!(
            scissor_at(&writes),
            Some(Scissor {
                x: 0,
                y: 0,
                width: 64,
                height: 64,
            })
        );

        // Only the top-left was written: no complete rectangle.
        assert!(
            scissor_at(&[write(0xA090, 0x8000_0000)]).is_none(),
            "the bottom-right corner was never written"
        );
    }
}
