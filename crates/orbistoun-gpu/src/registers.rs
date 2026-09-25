//! Register writes, and the shader addresses hiding among them.
//!
//! # The link between the two halves of the GPU work
//!
//! A submission does not contain shaders. It contains *addresses* of shaders, written
//! into hardware registers by ordinary register-write packets. Extracting those
//! addresses is what lets a captured command stream feed the shader corpus, and
//! without it the packet walker and the shader decoder are two tools that never meet.
//!
//! # Mechanism and interpretation are separated on purpose
//!
//! Pulling register writes out of a packet stream is **structural**: a type-0 packet
//! writes consecutive registers from a base in its header, and a `SET_*_REG` packet
//! writes consecutive registers from an index in its first body word. That is correct
//! regardless of what any particular register means.
//!
//! Deciding that register `0x2C08` holds the low half of a fragment shader's address
//! is a **hypothesis**, and unlike the shader encoding table there is no reference
//! implementation to check it against cheaply. So it lives in
//! `data/packets.toml`, it is correctable without a rebuild, and what comes out is
//! reported as a *candidate* address rather than as a fact.
//!
//! Getting that separation right matters more than getting the hypothesis right: the
//! mechanism will still be correct when the table is fixed.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::packet::{PacketKind, PacketWalk};

/// Why a packet vocabulary could not be loaded.
///
/// Its own type rather than the backend's: a malformed data file and a backend that
/// cannot render something are unrelated failures, and sharing an error between them
/// would make a caller handle one while thinking about the other.
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
/// Called a candidate rather than an address because the register mapping it rests on
/// is unverified. Nothing follows one of these blindly.
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
    /// The registers a stage's shader address is written to - every register
    /// [`shader_candidates`] reads (worklog 844).
    pub fn shader_register_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.shader_registers.keys().copied()
    }

    /// Parses a vocabulary from TOML.
    pub fn load(toml_text: &str) -> Result<Self, VocabularyError> {
        let file: VocabularyFile =
            toml::from_str(toml_text).map_err(|e| VocabularyError::Malformed(e.to_string()))?;

        let mut shader_registers = BTreeMap::new();
        for entry in file.shader_address {
            // Anything that is not the low half is treated as the high half, and an
            // unrecognised value would silently become one. Refusing is better: a
            // typo here produces addresses with their halves swapped, which look
            // plausible and are wrong by four billion.
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

    /// A readable name for a type-3 opcode, or `None` if the table has no entry.
    ///
    /// `None` rather than a placeholder string: a report should be able to show the
    /// raw value for an opcode nobody has named yet, and an invented name would hide
    /// that the vocabulary has a gap.
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
    /// Exposed so a consumer can build a submission that names one, and so a report can
    /// show what the vocabulary claims to know. Read-only: this is a transcription and
    /// the least certain thing in the file, so it is worth being inspectable.
    pub fn shader_registers(&self) -> impl Iterator<Item = (&u32, &(String, bool))> {
        self.shader_registers.iter()
    }

    /// The register-writing opcode that reaches a given register, and its base.
    ///
    /// Several opcodes write registers, each to a different class with its own base, and
    /// which one reaches a register is decided by the register - not by which opcode
    /// happens to come first. Asking for "an opcode that writes registers" and using it
    /// for any register underflows the offset for every register below its base, which
    /// is a subtraction that happens to be checked here and would be a silently wrong
    /// packet anywhere else.
    ///
    /// The closest base at or below the register, so a register in two classes' ranges
    /// resolves to the nearer one.
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
                // Body word zero is the register offset; the rest are values. Only its low
                // sixteen bits are the offset: the indexed user-config form carries an index in
                // bits 31:28 (the GL cube capture writes 0x10000242 for VGT_PRIMITIVE_TYPE), and
                // adding the whole word would name a register that does not exist.
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
/// An address needs both halves. A stage with only one half seen is **skipped rather
/// than half-formed**: an address missing its high word points into the bottom four
/// gigabytes and would look like an ordinary low address rather than like a mistake.
///
/// Later writes win, because a submission legitimately rebinds a stage several times
/// and the last one before a draw is the one that mattered.
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
            // The program registers hold the address in 256-byte units. Measured, not
            // transcribed: the GL cube capture (tests/captures/agc-gl-cube-fw1240-a) writes
            // 0x02008f03 for the pixel shader the hardware fetched from 0x2008f0300.
            Some(ShaderCandidate {
                stage: stage.to_owned(),
                address: ((u64::from(high) << 32) | u64::from(low)) << 8,
                packet_offset: offset,
            })
        })
        .collect()
}

/// The latest write to each register before a point in a stream, kept as the stream is walked
/// forward (worklog 844).
///
/// Everything a draw reads of the register state - its shaders, blend, viewport transform and user
/// data - is the **latest** write to each register before the draw. Answering that per draw by
/// scanning every earlier write made preparing a submission quadratic in its draws: a GL frame of
/// hundreds of draws rescanned thousands of writes hundreds of times, more than a quarter of every
/// second a title ran. This walks the writes once, in order, and hands each draw the latest per
/// register - which the per-draw functions answer from exactly as they did from the whole stream.
#[derive(Debug)]
pub struct RegisterSweep<'a> {
    writes: &'a [RegisterWrite],
    next: usize,
    reached: u32,
    /// The index of each register's latest write, by register - a table rather than a map because
    /// a GL frame asks it tens of lookups per draw, for thousands of draws (worklog 854). Every
    /// register a packet can name fits: the highest base, uconfig's `0xC000`, plus the widest
    /// packet's count stays below [`SWEEP_REGISTERS`], and anything past it goes to `beyond`.
    table: SweepTable,
    beyond: BTreeMap<u32, usize>,
}

/// Registers the sweep's table covers directly.
const SWEEP_REGISTERS: usize = 1 << 16;

/// No write to a register yet, in a [`SweepTable`].
const UNWRITTEN: u32 = u32::MAX;

/// A sweep's register table, and the registers it has filled in.
///
/// **Reused, and cleared by what it touched** (worklog 853): a fresh table was a megabyte allocated
/// and zeroed twice a submission, for a stream that writes a few hundred registers - more time than
/// the lookups it serves. A sweep takes the spare one, and returns it with only its written entries
/// reset, so the next sweep starts from the same all-unwritten table a fresh one would be.
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
    /// gathering, sorting and rescanning a list per draw (worklog 854).
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
/// earlier packets (worklog 835).
///
/// A stream that changes a stage's program between draws - the open-toolchain GL context swaps
/// vertex programs by vertex size, 48, 64 or 80 bytes - has a different shader in force at each, and
/// the whole stream's last write names only the one the final draws ran.
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

/// Reads `length` bytes at `start` as little-endian words.
/// One draw a submission asked for.
///
/// **Named by the opcode, counted from the body, and neither is invented.** The opcode names
/// come from `data/packets.toml`, and obSCEne's own measurement of the builder that emits them
/// agrees: `DcbDrawIndexAuto` is twelve bytes with header `0xc0012d00`, which is opcode `0x2d`
/// and two body words, and `DcbSetNumInstances` is eight with `0xc0002f00`.
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
/// # What is read and what is not
///
/// The auto-indexed draw's **first** body word is its vertex count, and the instance-count
/// packet's is the number of instances. Both are corroborated rather than transcribed: the
/// captured GL cube stream carries `c0012d00 00000003 00000002` against `c0002f00 00000001`, so
/// it asks for three vertices and one instance per draw - and three is exactly what the guest's
/// own primitive shader declares it will emit (worklog 585).
///
/// The draw's initiator word says *how* the draw is issued rather than what it draws. Nothing
/// here reads it, because nothing here would know what to do with it, and reading a field into a
/// value nobody uses is how a wrong reading survives.
///
/// **The indexed draw is extracted too, from its own body.** This once refused it, believing its
/// count came from separate state - but the measured `DRAW_INDEX_2` builder
/// (`packet::build::draw_index_2`) shows the body is `[max_size, addr_lo, addr_hi, index_count,
/// initiator]`, so the index count is body word 3 and the index-buffer address is words 1-2, both
/// read back by obSCEne's own dump. So an indexed draw is self-contained: nothing is invented
/// reading it, and a game that draws indexed geometry - the common case - is no longer counted as
/// having issued no draws at all.
pub fn draw_calls(walk: &PacketWalk, body: &[u8]) -> Vec<DrawCall> {
    let mut draws = Vec::new();
    // State, in the order the stream sets it: a draw takes the instance count most recently
    // written before it. One is the value a stream that never says means, and every draw in the
    // captured one is preceded by a packet saying so anyway.
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
                // Body `[max_size, addr_lo, addr_hi, index_count, initiator]`: the address is
                // words 1-2 and the count is word 3, both measured. A truncated body that lacks
                // them is dropped rather than read past - a short packet is a desync, not a draw.
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
/// packets and most are looked at for a word or two, so none of them is copied out (worklog 854).
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
/// `DISPATCH_DIRECT`'s body is `[x, y, z, initiator]` (the dispatch builder, worklog 534) - the
/// three workgroup counts, then a launch flag this does not read. A body too short to hold the three
/// counts is dropped rather than read past, the same as a truncated draw.
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

/// A draw or dispatch, paired with the shader addresses a capture shows were live when it issued.
///
/// This is the association that makes a capture *answerable* rather than merely readable (the glue
/// oops-libs asked for, `REQ-26aa`): a stream on its own is a flat list of register writes and draws,
/// and the question worth asking - which shaders did *this* draw run - is the join between them. The
/// shaders are those [`shader_candidates`] reassembles from every register write that precedes the
/// work, the most-recent-per-register rule used throughout this file, so a stage rebound between two
/// draws is attributed to each correctly.
///
/// The shader-address register map is a hypothesis with no oracle (D091), so these stay `candidates` -
/// reported, never dispatched on. The correlation inherits that caution rather than adding to it, and
/// carries no descriptor-table pointer: no register mapping for one is measured, so attaching a guess
/// would be exactly the plausible-output this file refuses (D010).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawCorrelation {
    /// The draw or dispatch, with its own decoded fields.
    pub work: DrawOrDispatch,
    /// The shader addresses live when it issued; empty when the stream bound none before it.
    pub shaders: Vec<ShaderCandidate>,
}

/// Correlates every draw and dispatch in a stream with the shader addresses live at it.
///
/// Reads the whole stream once into register writes, draws and dispatches, then - for each unit of
/// work, ordered by where it sits in the stream - reassembles the shader candidates from the writes
/// that precede it. A capture read from a file walks straight into this (`walk(&bytes)` then
/// `correlate_draws`), which is the plumbing that turns a hardware dump into per-draw answers rather
/// than a flat listing.
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
/// This is the concrete-value counterpart of `orbistoun_translate`'s `read_buffer_resource`, which
/// emits the same decode as *arithmetic* because a shader loads the descriptor at run time. Here the
/// four dwords are the ones a guest wrote into its scalar registers, so their values are known and
/// the fields come out as numbers - the base address, size and stride a host needs to make the
/// guest's buffer resident and bind it.
///
/// # Layout
///
/// From the descriptor table in the instruction-set reference: base address in bits 47:0, stride in
/// 61:48, record count in 95:64, swizzle-enable at bit 63 and add-thread-id at bit 119. Those land
/// across the four dwords as the shifts below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferDescriptor {
    /// Byte address of the buffer's base.
    pub base: u64,
    /// Bytes per record, `0..=0x3FFF`. Zero is a raw buffer whose `records` is a byte count.
    pub stride: u32,
    /// Records when there is a stride, otherwise bytes. See [`BufferDescriptor::byte_len`].
    pub records: u32,
    /// Whether the descriptor asks for addressing this project does not model - swizzled records or
    /// add-thread-id. A host cannot honour those by binding a plain range, so it is a refusal, not a
    /// buffer.
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
/// up is not one to bind. The most recent write to each register wins, which is the value that would
/// be live when the shader read it.
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
/// The width and height a draw rasterises into. A host needs them to size the attachment a
/// translated pipeline draws to: without them a frame is drawn at a guessed size, and a guessed
/// size draws every pixel in the wrong place while looking like it worked.
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
/// `0x3B0`. Measured, not transcribed - the GL cube capture (tests/captures/agc-gl-cube-fw1240-a)
/// writes `0x01dfc437` here for the frame the console hashed, and [`decode_colour_target_extent`]
/// turns that into the `1920 x 1080` the record states. The decode is self-checking: no other pair
/// comes out of that value.
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

/// The colour target's dimensions, from the live value of `CB_COLOR0_ATTRIB2` among the writes.
///
/// [`None`] when the stream never sets it. A target size this would otherwise have to guess is one
/// it refuses to guess (D010) - drawing at a fabricated size is the plausible-output failure, not a
/// smaller version of the right one. The most recent write wins, the value that would be live at the
/// draw, the same rule [`buffer_descriptor_at`] and [`shader_candidates`] use.
#[must_use]
pub fn colour_target_extent_at(writes: &[RegisterWrite]) -> Option<ColourTargetExtent> {
    let value = writes
        .iter()
        .rev()
        .find(|write| write.register == CB_COLOR0_ATTRIB2)?
        .value;
    Some(decode_colour_target_extent(value))
}

/// The register that carries colour buffer zero's base address.
///
/// `CB_COLOR0_BASE`, a context register: `SET_CONTEXT_REG` base `0xA000` plus offset `0x318`
/// (register index `0xA318`). Measured against oops-mesa's register database, not transcribed from
/// memory: `oops-mesa src/amd/registers/gfx103.json` maps `CB_COLOR0_BASE` at byte `167008` =
/// `0x28C60`, which is context dword `(0x28C60 - 0x28000) / 4` = `0x318`. The value is the address in
/// 256-byte units - the same unit [`ImageDescriptor`]'s base uses - so the byte address is
/// `value << 8`.
const CB_COLOR0_BASE: u32 = 0xA318;

/// Colour buffer zero, as a submission set it up: where it is and how big.
///
/// The address, width and height a host needs to make the render target resident - and the base is
/// what [`crate::tiling`] slices before detiling, so a captured frame's pixels can be read out of
/// guest memory. It pairs the two registers a target minimally needs: its base and its extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColourTarget {
    /// Base byte address of the surface in guest memory.
    pub base: u64,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// The colour target a submission set up, from the live values of `CB_COLOR0_BASE` and
/// `CB_COLOR0_ATTRIB2` among the writes.
///
/// [`None`] unless both the base and the extent were set: a target missing either is one a host can
/// neither place nor size, and inventing the missing half is the plausible-output this refuses (D010).
/// The most recent write to each wins - the value live at the draw, the rule
/// [`colour_target_extent_at`] and [`buffer_descriptor_at`] use.
#[must_use]
pub fn colour_target_at(writes: &[RegisterWrite]) -> Option<ColourTarget> {
    let base = writes
        .iter()
        .rev()
        .find(|write| write.register == CB_COLOR0_BASE)?
        .value;
    let extent = colour_target_extent_at(writes)?;
    Some(ColourTarget {
        base: u64::from(base) << 8,
        width: extent.width,
        height: extent.height,
    })
}

/// The register that carries which colour targets are written, and on which channels.
///
/// `CB_TARGET_MASK`, a context register: `SET_CONTEXT_REG` base `0xA000` plus offset `0x08E`
/// (register index `0xA08E`). Cited from oops-mesa, not memory: `oops-mesa
/// src/amd/registers/gfx103.json` maps `CB_TARGET_MASK` at byte `164408` = `0x28238`, context dword
/// `(0x28238 - 0x28000) / 4` = `0x8E`, and defines its eight four-bit fields
/// `TARGET0_ENABLE`..`TARGET7_ENABLE` at bits `[0,3]`..`[28,31]`.
const CB_TARGET_MASK: u32 = 0xA08E;

/// Which of the eight colour targets a submission writes, and on which channels.
///
/// One four-bit channel mask per MRT (bit 0 red, 1 green, 2 blue, 3 alpha), so `0xF` writes all four
/// and `0` disables the target. A host needs it to know how many attachments to configure and which
/// components a draw writes - a target left out of the mask is not rendered to.
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

/// The colour write mask a submission set, from the live value of `CB_TARGET_MASK` among the writes.
///
/// [`None`] when the stream never sets it - which targets are written is not a thing to assume, so an
/// unset mask is reported as absent rather than defaulted to "all" or "none". Most-recent-write-wins.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// `DB_DEPTH_CONTROL`, a context register: `SET_CONTEXT_REG` base `0xA000` plus offset `0x200`
/// (register index `0xA200`). Cited from oops-mesa, not memory: `oops-mesa
/// src/amd/registers/gfx103.json` maps `DB_DEPTH_CONTROL` at byte `165888` (`165888 / 4` = `0xA200`,
/// context dword `0x200`) and defines its fields.
const DB_DEPTH_CONTROL: u32 = 0xA200;

/// The depth- and stencil-test state a draw runs under, decoded from `DB_DEPTH_CONTROL`.
///
/// What a host needs to configure a depth-stencil pipeline: whether each test is on, whether depth is
/// written, and the comparison each uses. The two colour-write-on-depth-{fail,pass} interaction bits
/// (30, 31) are a rarer feature and left undecoded here - this covers the test state, which is what a
/// draw turns on.
// Each bool is one of `DB_DEPTH_CONTROL`'s independent hardware enable bits, named rather than folded
// into flags so a reader sees which test a value turned on; the "too many bools" lint does not fit a
// register mirror.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// `STENCILFUNC_BF` (bits 20:22): the back-face stencil comparison, used when `backface_enable`.
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
/// [`None`] when the stream never sets it - the test state is not a thing to assume, so an unset
/// register is reported as absent rather than defaulted. Most-recent-write-wins.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// `DB_STENCIL_CONTROL`, a context register: `SET_CONTEXT_REG` base `0xA000` plus offset `0x10B`
/// (register index `0xA10B`). Cited from oops-mesa, not memory: `oops-mesa
/// src/amd/registers/gfx103.json` maps `DB_STENCIL_CONTROL` at byte `164908` (`164908 / 4` = `0xA10B`)
/// and defines its six four-bit `StencilOp` fields.
const DB_STENCIL_CONTROL: u32 = 0xA10B;

/// The stencil operations a draw applies on each test outcome, decoded from `DB_STENCIL_CONTROL`.
///
/// Front and back faces each carry three operations: what to do when the stencil test fails, when it
/// passes and the depth test passes, and when it passes but the depth test fails. The back-face set
/// applies only when `DB_DEPTH_CONTROL`'s back-face stencil is on ([`DepthControl::backface_enable`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// A blend factor - the `BlendOp` enum each source/destination field of `CB_BLEND0_CONTROL` selects.
///
/// What a colour or alpha channel is multiplied by before the combine function. Cited from oops-mesa
/// `src/amd/registers/gfx103.json` (`BlendOp`: `BLEND_ZERO` 0 .. `BLEND_ONE_MINUS_CONSTANT_ALPHA` 20).
/// The field is five bits and `21..32` are reserved, carried as [`BlendFactor::Other`] rather than
/// mapped to a defined factor.
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
/// `src/amd/registers/gfx103.json` (`CombFunc`: `COMB_DST_PLUS_SRC` 0 .. `COMB_DST_MINUS_SRC` 4); the
/// field is three bits and `5..8` are reserved, carried as [`CombineFunc::Other`].
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

/// `CB_BLEND0_CONTROL`, a context register: `SET_CONTEXT_REG` base `0xA000` plus offset `0x1E0`
/// (register index `0xA1E0`). Cited from oops-mesa, not memory: `oops-mesa
/// src/amd/registers/gfx103.json` maps `CB_BLEND0_CONTROL` at byte `165760` (`165760 / 4` = `0xA1E0`)
/// and defines its fields.
pub(crate) const CB_BLEND0_CONTROL: u32 = 0xA1E0;

/// The colour-blend state for colour target zero, decoded from `CB_BLEND0_CONTROL`.
///
/// A source and destination factor and a combine function for colour, the same three for alpha, and
/// the flags that turn blending on and let alpha use its own set. What a host needs to build a colour
/// blend attachment. It is colour target zero only; targets `1..8` have their own `CB_BLENDn_CONTROL`.
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
/// `CB_COLOR0_ATTRIB3`, a context register: `SET_CONTEXT_REG` base `0xA000` plus offset `0x3B8`
/// (register index `0xA3B8`). Cited from oops-mesa: `oops-mesa src/amd/registers/gfx103.json` maps it
/// at byte `167648` = `0x28EE0`, context dword `(0x28EE0 - 0x28000) / 4` = `0x3B8`.
const CB_COLOR0_ATTRIB3: u32 = 0xA3B8;

/// `COLOR_SW_MODE` value for a linear (un-swizzled) surface: `ADDR_SW_LINEAR`.
///
/// From `oops-mesa src/amd/addrlib/inc/addrtypes.h:227` (`AddrSwizzleMode`).
const ADDR_SW_LINEAR: u32 = 0;

/// `COLOR_SW_MODE` value for the 64KB_R_X tiling that [`crate::tiling`] detiles: `ADDR_SW_64KB_R_X`.
///
/// From `oops-mesa src/amd/addrlib/inc/addrtypes.h:254` (`AddrSwizzleMode`).
const ADDR_SW_64KB_R_X: u32 = 27;

/// The tiling (swizzle) mode of a surface - a colour target or a texture.
///
/// The mode decides whether, and how, a surface's pixels are swizzled in guest memory. Named here are
/// only the two orbistoun can act on: linear (read straight) and 64KB_R_X (the one [`crate::tiling`]
/// detiles). Any other mode is reported by its raw five-bit value rather than given an invented name,
/// so an unhandled tiling is refused downstream (D010) instead of read with the wrong swizzle. The
/// values are `AddrSwizzleMode` (`oops-mesa src/amd/addrlib/inc/addrtypes.h:225`), shared by the
/// colour target's `COLOR_SW_MODE` and a texture's `SQ_IMG_RSRC_WORD3.SW_MODE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwizzleMode {
    /// No swizzle; pixels are row-major (`ADDR_SW_LINEAR`).
    Linear,
    /// 64KB_R_X, the mode [`crate::tiling`] detiles (`ADDR_SW_64KB_R_X`).
    Tiled64KbRX,
    /// A mode orbistoun does not model, carried by its raw five-bit swizzle-mode value.
    Other(u32),
}

/// Decodes a five-bit `SW_MODE` field into a [`SwizzleMode`].
///
/// The field is the raw value a `COLOR_SW_MODE` or `SQ_IMG_RSRC_WORD3.SW_MODE` carries once the caller
/// has shifted it out of its register. Values are `AddrSwizzleMode`: linear `0`, 64KB_R_X `27`.
#[must_use]
pub fn decode_swizzle_mode(field: u32) -> SwizzleMode {
    match field & 0x1F {
        ADDR_SW_LINEAR => SwizzleMode::Linear,
        ADDR_SW_64KB_R_X => SwizzleMode::Tiled64KbRX,
        other => SwizzleMode::Other(other),
    }
}

/// Decodes `CB_COLOR0_ATTRIB3`'s `COLOR_SW_MODE` (bits 18:14) into a swizzle mode.
///
/// The field bits are from `oops-mesa src/amd/registers/gfx103.json` (the `CB_COLOR0_ATTRIB3` type,
/// `COLOR_SW_MODE` at bits `[14, 18]`); the `-a1f7` capture's `0x08c6c000` decodes to `27` = 64KB_R_X,
/// which is the mode the detile handles and oops-sdk's gl-cube work independently recorded.
#[must_use]
pub fn decode_colour_swizzle_mode(attrib3: u32) -> SwizzleMode {
    decode_swizzle_mode((attrib3 >> 14) & 0x1F)
}

/// The colour target's tiling mode, from the live value of `CB_COLOR0_ATTRIB3` among the writes.
///
/// [`None`] when the stream never sets it - a surface whose tiling is unknown must not be read as
/// either linear or tiled, so an unset mode is absent rather than defaulted. Most-recent-write-wins.
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
/// `YSCALE`, `YOFFSET` - `gfx103.json` maps them at bytes `164924`..`164936`, dwords `0xA10F`..`0xA112`
/// (worklog 837).
const PA_CL_VPORT_XSCALE: u32 = 0xA10F;
/// `PA_CL_VTE_CNTL` (`gfx103.json`, byte `165912`, dword `0xA206`): bits 0-3 enable the x/y scale and
/// offset.
const PA_CL_VTE_CNTL: u32 = 0xA206;
/// `VPORT_X_SCALE_ENA` through `VPORT_Y_OFFSET_ENA`.
const VTE_XY_ENABLES: u32 = 0xF;

/// The viewport transform a draw's clip-space positions are mapped to the target with (worklog 837):
/// `x = x_scale * ndc_x + x_offset`, `y = y_scale * ndc_y + y_offset`, in the target's pixels, rows
/// growing down.
///
/// **The sign is the point.** The open-toolchain GL context writes `YSCALE = -height / 2` - GL's NDC
/// `+y` is up and the target's rows grow down (oops-sdk `gl_internal.h`, `gl_compute_vport`). A host
/// that ignores the transform and maps `+y` down draws every frame upside down.
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
}

/// The viewport transform in force at packet `before`, from the last write of each of the four
/// `PA_CL_VPORT_*` registers in an earlier packet (worklog 837).
///
/// `None` when any of the four was never written - a transform missing a term is not guessed at - or
/// when `PA_CL_VTE_CNTL` was written without all four x/y enables, since then the hardware does not
/// apply them and this has not modelled what it does instead. Both SDK draw paths write `0x43f`.
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

/// [`viewport_transform_at`], reading each register's value through `last` - a
/// [`RegisterSweep::latest`] for a caller walking draws in order (worklog 854).
pub fn viewport_transform_from(
    mut last: impl FnMut(u32) -> Option<u32>,
) -> Option<ViewportTransform> {
    if last(PA_CL_VTE_CNTL).is_some_and(|value| value & VTE_XY_ENABLES != VTE_XY_ENABLES) {
        return None;
    }
    Some(ViewportTransform {
        x_scale: f32::from_bits(last(PA_CL_VPORT_XSCALE)?),
        x_offset: f32::from_bits(last(PA_CL_VPORT_XSCALE + 1)?),
        y_scale: f32::from_bits(last(PA_CL_VPORT_XSCALE + 2)?),
        y_offset: f32::from_bits(last(PA_CL_VPORT_XSCALE + 3)?),
    })
}

/// `CB_COLOR0_INFO`, a context register: `gfx103.json` maps it at byte `167024`, dword `0xA31C`
/// (worklog 832).
const CB_COLOR0_INFO: u32 = 0xA31C;

/// `ColorFormat` `COLOR_8_8_8_8` (`gfx103.json`, enum `ColorFormat`).
pub const COLOR_8_8_8_8: u32 = 10;
/// `SurfaceNumber` `NUMBER_UNORM` (`gfx103.json`, enum `SurfaceNumber`).
pub const NUMBER_UNORM: u32 = 0;

/// Which memory byte each of a four-channel colour target's shader outputs lands in -
/// `CB_COLOR0_INFO.COMP_SWAP` (worklog 832).
///
/// The two orders named are the ones Mesa gives a four-channel format (`ac_formats.c:614-619`):
/// `SWAP_STD` is `XYZW` - memory holds R, G, B, A - and `SWAP_ALT` is `ZYXW` - memory holds B, G, R, A,
/// the order the open-toolchain GL context's display targets use (oops-sdk `gl_draw.c`, "COMP_SWAP=ALT
/// (bytes B,G,R,A)"). The reversed orders are carried raw and refused where a byte order is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentSwap {
    /// `SWAP_STD` (0): memory order R, G, B, A.
    Standard,
    /// `SWAP_ALT` (1): memory order B, G, R, A.
    Alternate,
    /// `SWAP_STD_REV` (2) or `SWAP_ALT_REV` (3), by raw value.
    Other(u32),
}

/// Colour target zero's element layout: `CB_COLOR0_INFO`'s `FORMAT` (bits 2-6), `NUMBER_TYPE` (8-10)
/// and `COMP_SWAP` (11-12), field bits from `oops-mesa src/amd/registers/gfx103.json`.
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

/// Decodes a `CB_COLOR0_INFO` value. The SDK's measured primitive-draw value `0x000180a8` decodes to
/// `8_8_8_8`, `UNORM`, `SWAP_STD`.
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

/// Colour target zero's element layout from the live `CB_COLOR0_INFO` among the writes; [`None`] when
/// the stream never set it. Most-recent-write-wins.
#[must_use]
pub fn colour_target_format_at(writes: &[RegisterWrite]) -> Option<ColourTargetFormat> {
    let value = writes
        .iter()
        .rev()
        .find(|write| write.register == CB_COLOR0_INFO)?
        .value;
    Some(decode_colour_target_format(value))
}

/// How many distinct colour target zero base addresses a stream wrote. A submission whose draws are
/// carried out together and written back as one frame (worklog 832) must have drawn into one target,
/// so more than one is a refusal, not a guess at which draw went where.
#[must_use]
pub fn colour_target_bases_in(writes: &[RegisterWrite]) -> usize {
    let mut bases: Vec<u32> = writes
        .iter()
        .filter(|write| write.register == CB_COLOR0_BASE)
        .map(|write| write.value)
        .collect();
    bases.sort_unstable();
    bases.dedup();
    bases.len()
}

/// The absolute dword index of `VGT_GS_OUT_PRIM_TYPE`.
///
/// From `oops-mesa src/amd/registers/gfx103.json` (`"map": {"at": 166508}`; `166508 / 4` = `0xA29B`),
/// a context-space register. Measured against the captures: the point record
/// (`agc-primitive-draw-fw1240`) writes `0` (POINTLIST), the triangle and gl-cube records `2`
/// (TRISTRIP).
const VGT_GS_OUT_PRIM_TYPE: u32 = 0xA29B;

/// The primitive a draw's geometry produces, from `VGT_GS_OUT_PRIM_TYPE.OUTPRIM_TYPE`.
///
/// This is the **output** primitive type - what the geometry stage emits and the rasteriser
/// assembles - which is the topology a draw actually produces. It is the register that tells a point
/// draw from a triangle one: the input-assembly `VGT_PRIMITIVE_TYPE` reads `TRILIST` for the point and
/// the triangle records alike, while this reads POINTLIST for the one and TRISTRIP for the other.
///
/// Only the values with a meaning here are named; anything else is carried by its raw field so an
/// unhandled topology is refused downstream rather than assembled as something it is not (D010, the
/// same rule [`SwizzleMode`] follows). `RectangleList` is named but has no graphics-primitive
/// analogue, so it too is for a consumer to refuse by name rather than draw as triangles.
///
/// Values are `VGT_GS_OUTPRIM_TYPE` (`oops-mesa src/amd/registers/gfx103.json`): POINTLIST 0,
/// LINESTRIP 1, TRISTRIP 2, RECTLIST 3.
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
/// The field bits are from `oops-mesa src/amd/registers/gfx103.json` (`VGT_GS_OUT_PRIM_TYPE` type,
/// `OUTPRIM_TYPE` at bits `[0, 5]`); the values are its `VGT_GS_OUTPRIM_TYPE` enum.
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
/// [`None`] when the stream never sets it - a draw whose topology is unknown must not be assumed a
/// triangle list, which is exactly what the backend's mesh output does today (`-0c58`).
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
/// From `oops-mesa src/amd/registers/gfx103.json` (`"map": {"at": 166740}`; `166740 / 4` = `0xA2D5`),
/// a context-space register. `GS_W32_EN` is its bit 22 in the same file's `VGT_SHADER_STAGES_EN`
/// type: the primitive shader - what the host runs as a mesh stage - is thirty-two lanes wide.
const VGT_SHADER_STAGES_EN: u32 = 0xA2D5;
const GS_W32_EN: u32 = 1 << 22;

/// The absolute dword index of `SPI_PS_IN_CONTROL`.
///
/// From `oops-mesa src/amd/registers/gfx103.json` (`"map": {"at": 165592}`; `165592 / 4` = `0xA1B6`).
/// `PS_W32_EN` is its bit 15: the pixel shader is thirty-two lanes wide.
const SPI_PS_IN_CONTROL: u32 = 0xA1B6;
const PS_W32_EN: u32 = 1 << 15;

/// Which of a draw's stages run thirty-two lanes wide.
///
/// The encodings are the same at either width and nothing in the instruction stream says which a
/// shader was compiled for (D141): the hardware is told, in these two registers. A clear bit - or a
/// register the stream never wrote, whose reset value is zero - is the sixty-four-lane wave.
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
/// The concrete-value counterpart of the sampled image the translator emits as *arithmetic* - it
/// reads none of these fields, because the GPU reads the descriptor at run time
/// (`orbistoun_translate::model`, `IMAGE_DESCRIPTOR_REGISTERS`) - the same way [`BufferDescriptor`]
/// is the concrete counterpart of a V#. Here the eight dwords are the ones a guest wrote, so the
/// texture's dimensions, format and base come out as the numbers a host needs to make it resident.
///
/// # Layout
///
/// From the GFX10 image resource descriptor: base address in dword 0 (in 256-byte units) plus the low
/// byte of dword 1; format in dword 1 bits 28:20; and the extent stored one below its size - the
/// width split across dword 1 bits 31:30 (its low two bits) and dword 2 bits 11:0, the height in
/// dword 2 bits 27:14. Measured against the open-source driver's own construction: oops-mesa
/// `src/amd/registers/gfx10-rsrc.json` (`SQ_IMG_RSRC_WORD1`/`WORD2`) and
/// `src/amd/common/ac_descriptors.c:ac_build_gfx10_texture_descriptor`, which writes
/// `S_00A008_WIDTH_HI((width - 1) >> 2)` against `S_00A004_WIDTH_LO(width - 1)`.
///
/// # The tiling mode, and what still waits
///
/// The tiling *mode* is decoded - `SQ_IMG_RSRC_WORD3.SW_MODE` (dword 3 bits 24:20, oops-mesa
/// `src/amd/registers/gfx10-rsrc.json`), the same `AddrSwizzleMode` a colour target carries - so a
/// caller can tell whether [`crate::tiling`] can detile the pixels ([`SwizzleMode::Tiled64KbRX`]),
/// they are linear, or the mode is one to refuse. The swizzle *equation* is measured only for
/// 64KB_R_X at 32 bpp (worklog 653); block-compressed formats and other modes still wait on theirs
/// (G15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageDescriptor {
    /// Byte address of the texture's pixels.
    pub base: u64,
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// The image format, as the GFX10 format code. Reported, not interpreted: a host maps it to its
    /// own format, and an unrecognised one is a refusal there rather than a guess here.
    pub format: u32,
    /// The tiling mode - which swizzle, if any, the pixels are stored under. `Tiled64KbRX` is the one
    /// [`crate::tiling`] detiles; `Linear` reads straight; `Other` is a mode to refuse rather than
    /// read with the wrong swizzle.
    pub tiling: SwizzleMode,
}

/// A scissor rectangle - the region a guest restricts rasterisation to - decoded from the
/// `GENERIC_SCISSOR` top-left and bottom-right register pair.
///
/// The application scissor a `SetViewport` carries (D010): a draw paints only inside it. Its
/// coordinates are non-negative pixels in the target, so all four fields are unsigned; a caller that
/// wants the command's signed rectangle widens `x` and `y`.
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
/// Context registers `0xA090` and `0xA091` (`SET_CONTEXT_REG` base `0xA000` plus offsets `0x090` and
/// `0x091`). Measured: the obSCEne draw oracle (`166-agc/primitive-draw`, sweep `20260916-223136`)
/// writes `0x80000000`/`0x00400040` here for its 64x64 frame, which decodes to `(0,0)`-`(64,64)`. The
/// generic scissor is the application-controlled one, as opposed to the screen, window and per-viewport
/// scissors the same draw also sets to the whole target.
const PA_SC_GENERIC_SCISSOR_TL: u32 = 0xA090;
/// See [`PA_SC_GENERIC_SCISSOR_TL`].
const PA_SC_GENERIC_SCISSOR_BR: u32 = 0xA091;

/// Decodes a scissor rectangle from its top-left and bottom-right register values.
///
/// The layout is `PA_SC_WINDOW_SCISSOR_TL`/`_BR` (oops-mesa `src/amd/registers/gfx10.json`), which the
/// generic scissor shares: x in bits 14:0, y in bits 30:16, and on the top-left a
/// `WINDOW_OFFSET_DISABLE` flag at bit 31 that is not part of the rectangle and is masked off. The
/// width and height are the corners' difference, clamped at zero so a bottom-right above the top-left
/// is an empty rectangle rather than an underflow.
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

/// The scissor from the live `GENERIC_SCISSOR` writes, or [`None`] when the stream set only one corner
/// or neither.
///
/// Both corners are needed for a rectangle; a stream that wrote one and not the other is not one to
/// guess the rest of (D010). The most recent write to each wins, the value live at the draw.
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
/// The bit positions are those [`ImageDescriptor`] documents. The first three dwords carry the base,
/// format and extent and dword 3 the tiling mode; the rest hold mip levels and the array bounds this
/// does not model yet.
#[must_use]
pub fn decode_image_descriptor(words: [u32; 8]) -> ImageDescriptor {
    // dword 1 (`mid`) carries the base's high byte, the format and the width's low bits; dword 2
    // (`extent`) the width's high bits and the height; dword 3 (`tiling_word`) the tiling mode.
    let [base_low, mid, extent, tiling_word, ..] = words;
    let base = (u64::from(base_low) | (u64::from(mid & 0xFF) << 32)) << 8;
    // Width minus one: low two bits in dword 1 bits 31:30, the rest in dword 2 bits 11:0.
    let width = (((mid >> 30) & 0x3) | ((extent & 0xFFF) << 2)) + 1;
    let height = ((extent >> 14) & 0x3FFF) + 1;
    ImageDescriptor {
        base,
        width,
        height,
        format: (mid >> 20) & 0x1FF,
        // Tiling mode: dword 3 bits 24:20 (`SQ_IMG_RSRC_WORD3.SW_MODE`, oops-mesa gfx10-rsrc.json).
        tiling: decode_swizzle_mode((tiling_word >> 20) & 0x1F),
    }
}

#[cfg(test)]
mod tests {

    /// **The sweep answers what the whole stream would** (worklog 854): the latest write before a
    /// packet, per register, asked forwards, asked backwards (which starts again), and for a
    /// register past the table.
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

    /// **A sweep starts from nothing, whatever the sweep before it on this thread saw** (worklog
    /// 853): the table is reused, so a register the last stream wrote and this one does not must
    /// read unwritten here, never the other stream's value.
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

    /// **Each stage's width comes from its own bit** (worklog 854): the GL context's NGG setup
    /// (`VGT_SHADER_STAGES_EN` = `0x00c12010`, `oops-sdk src/gl/gl_draw.c`) sets `GS_W32_EN`, and its
    /// `SPI_PS_IN_CONTROL` sets `PS_W32_EN`. The AGC fixture's `0x02002000` sets neither, and a stream
    /// that writes neither register is the reset value's sixty-four lanes. The later write wins.
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

    /// **`CB_COLOR0_INFO` decodes to the element layout a written-back frame needs** (worklog 832): the
    /// SDK's measured primitive-draw value is `8_8_8_8` `UNORM` in standard order, and the same value
    /// with `COMP_SWAP` = 1 - the GL context's display targets - is the B, G, R, A order. A reversed
    /// order or another format is not one a frame is written into.
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
        // D091 calls the shader-address register map "a hypothesis with no oracle", and
        // for the register *numbers* that is still true - they are transcribed, and the
        // instruction-set reference names the registers without giving their offsets.
        //
        // It does say something checkable about their *shape*, though: a shader's start
        // address comes from `SPI_SHADER_PGM_LO/HI`, per stage, as a pair. So each stage
        // must have exactly one of each half and the high must sit immediately above the
        // low. A transposed digit or a dropped half breaks that, and those are the
        // transcription mistakes this table is most exposed to.
        //
        // It does not make the numbers right. It makes one class of being wrong loud.
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
    use super::{BlendFactor, CombineFunc, StencilOp};
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
        // The index is data, not header. Reading it as a value instead would write the
        // register number into a register and shift everything by one.
        let bytes = stream(&[command(0x76, 3), 0x0C, 0x1111, 0x2222]);
        let writes = register_writes(&walk(&bytes), &bytes, &vocabulary());
        assert_eq!(writes.len(), 2, "the index word is not a value");
        assert_eq!(writes[0].register, 0x2C0C, "base plus index");
        assert_eq!(writes[0].value, 0x1111);
        assert_eq!(writes[1].register, 0x2C0D);
    }

    #[test]
    fn a_packet_the_vocabulary_does_not_know_writes_nothing() {
        // Guessing a base for an unknown opcode would attribute its body to registers
        // that were never written, which is worse than recording nothing.
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
        // An address missing its high word points into the bottom four gigabytes and
        // reads as an ordinary low address rather than as a mistake. Skipping it keeps
        // a truncated stream from producing a plausible wrong answer.
        let bytes = stream(&[command(0x76, 2), 0x0C, 0x8000_0000]);
        let walked = walk(&bytes);
        let writes = register_writes(&walked, &bytes, &vocabulary());
        assert!(shader_candidates(&writes, &vocabulary()).is_empty());
    }

    #[test]
    fn a_later_bind_replaces_an_earlier_one() {
        // A submission rebinds a stage several times; the last write before a draw is
        // the one that mattered.
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

    /// **The viewport transform in force at a draw is its four registers' last writes before it**
    /// (worklog 837): the GL context's 1080p transform has a negative y scale - its NDC `+y` is up -
    /// and a draw before the writes, or after a `VTE_CNTL` disabling the x/y terms, has none.
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

    /// **The vertex program is read from `PGM_LO/HI_ES`, not `PGM_LO/HI_VS`** (worklog 836): on this
    /// generation the NGG wave takes its program counter from ES (Mesa `radv_shader.c:2078-2084`), and
    /// the GL context's mid-frame program switch writes only the GS and ES pairs. A stream that points
    /// ES at one program and VS at another names the ES one.
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

    /// **Each draw runs the shader bound before it, not the stream's last** (worklog 835): the GL
    /// context swaps a stage's program between draws, and a draw between the two binds saw the first.
    /// Before this, every draw of a Neverball frame ran whichever vertex program was written last.
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
        // Bind fragment A, draw; rebind fragment B, draw. Each draw must carry its own bind, not
        // the other's - the join a capture is read for.
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
        // A bind that comes AFTER the draw is not live at it. Without the before-this-point filter
        // this draw would be falsely attributed to the later shader - the guard for that join.
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
        // The plumbing REQ-26aa asked for, end to end: a dword stream on disk, read back, walked,
        // and reported as a draw carrying the shader address live when it issued.
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
        // Defaulting an unrecognised value to "high" would swap the halves of every
        // address it touched - plausible-looking values, wrong by four billion.
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
        // An unnamed opcode reports as unnamed rather than inventing a label, so a gap
        // in the vocabulary stays visible in a report.
        assert_eq!(vocabulary.opcode_name(0xFE), None);
    }

    /// **An auto draw carries its vertex count, and takes the instance count set before it.**
    #[test]
    fn an_auto_draw_reads_its_vertices_and_the_running_instance_count() {
        // NUM_INSTANCES then DRAW_INDEX_AUTO, the shape the captured GL cube uses.
        let bytes = stream(&[command(0x2F, 1), 2, command(0x2D, 2), 3, 0]);
        let draws = draw_calls(&walk(&bytes), &bytes);
        assert_eq!(draws.len(), 1);
        assert_eq!(draws[0].instances, 2);
        assert_eq!(draws[0].kind, DrawKind::Auto { vertices: 3 });
    }

    /// **An indexed draw is read from its own body, address and count both.**
    ///
    /// The measured `DRAW_INDEX_2` body is `[max_size, addr_lo, addr_hi, index_count, initiator]`,
    /// so the count and the index-buffer address are in the packet - not separate state. Made to
    /// fail against the code that dropped indexed draws entirely: it produced no `DrawCall` here.
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

    /// **A truncated indexed draw is dropped, not read past its body.**
    ///
    /// A `DRAW_INDEX_2` whose body is short of the count word is a desync, and reading the missing
    /// dword would fabricate an index count from whatever followed the packet.
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

    /// **A buffer descriptor's fields land in the bits the reference gives them.**
    ///
    /// Base spans dword 0 and the low half of dword 1; stride is the next fourteen bits; the record
    /// count is dword 2. Made to fail by asserting each field against a descriptor built to a known
    /// base, stride and count.
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

    /// **A raw buffer's record count is a byte count, and unmodelled addressing is refused.**
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

    /// **A descriptor is read from the live values of four consecutive registers.**
    ///
    /// The most recent write to each register wins, and a descriptor missing any of its four
    /// registers is not one to bind.
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

    /// **A direct compute dispatch decodes its three workgroup counts.**
    ///
    /// `DISPATCH_DIRECT`'s body is `[x, y, z, initiator]`; the counts come out and the launch flag
    /// is left. A body too short to hold the three counts is no dispatch.
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

    /// **A colour target decodes its width and height from CB_COLOR0_ATTRIB2.**
    ///
    /// The measured GL cube value `0x01dfc437` is the `1920 x 1080` frame the console hashed: width
    /// minus one in bits 27:14, height minus one in bits 13:0. Made to fail against a decode that
    /// dropped the minus-one (would give 1919 x 1079), swapped the fields (1080 x 1920), or masked
    /// the wrong width - only this pair is right, and it is the frame the record states.
    #[test]
    fn a_colour_target_decodes_its_width_and_height() {
        assert_eq!(
            decode_colour_target_extent(0x01df_c437),
            ColourTargetExtent {
                width: 1920,
                height: 1080,
            }
        );

        // Zero in both fields is a one-by-one target: the stored value is one below the pixel count,
        // so a decode that forgot the minus-one would call this zero-by-zero.
        assert_eq!(
            decode_colour_target_extent(0),
            ColourTargetExtent {
                width: 1,
                height: 1,
            }
        );

        // The fields do not bleed into each other: a maximal width leaves the height at its minimum.
        assert_eq!(
            decode_colour_target_extent(0x3FFF << 14),
            ColourTargetExtent {
                width: 16384,
                height: 1,
            }
        );
    }

    /// **The extent is the live CB_COLOR0_ATTRIB2 write, and absent when the stream never sets it.**
    ///
    /// A stream that never sized its target offers no extent rather than a guessed one (D010). Made
    /// to fail against reading the first write instead of the last, and against inventing a size for
    /// a stream that set only unrelated registers.
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
            write(0xA318, 0x0200_0e00), // base -> 0x2000e0000 once shifted (the -a1f7 value)
            write(0xA3B0, 0x000f_c03f), // (63<<14)|63 -> 64x64
        ];
        assert_eq!(
            colour_target_at(&writes),
            Some(ColourTarget {
                base: 0x2_000e_0000,
                width: 64,
                height: 64,
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
        // BACKFACE_ENABLE; depth-bounds off. STENCIL_ENABLE bit 0, Z_ENABLE 1, Z_WRITE 2, ZFUNC 4:6,
        // BACKFACE 7, STENCILFUNC 8:10, STENCILFUNC_BF 20:22.
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
        // MAX(3), ONE_MINUS_SRC_ALPHA(5); alpha ONE(1), DST_MINUS_SRC(4), ZERO(0); separate-alpha and
        // enable on, ROP3 not disabled.
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
        // The -a1f7 capture's ATTRIB3 value decodes to 64KB_R_X, the mode the detile handles.
        assert_eq!(
            decode_colour_swizzle_mode(0x08c6_c000),
            SwizzleMode::Tiled64KbRX,
            "the measured -a1f7 tiling"
        );
        // COLOR_SW_MODE is bits 18:14, so field value 0 is linear whatever the other bits carry.
        assert_eq!(decode_colour_swizzle_mode(0x0000_0000), SwizzleMode::Linear);
        // An unmodelled mode is carried by its raw value, not mistaken for one we handle: 4KB_S is
        // ADDR_SW_4KB_S = 5, so `5 << 14` = 0x0001_4000 in the field.
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

    /// **An image descriptor decodes its base, dimensions and format from the GFX10 layout.**
    ///
    /// Built exactly as the open-source driver constructs one (`S_00A004_WIDTH_LO(w - 1)` against
    /// `S_00A008_WIDTH_HI((w - 1) >> 2)`, `S_00A008_HEIGHT(h - 1)`), so a decode that dropped the
    /// minus-one, mis-split the width, or masked the wrong bits disagrees. The 1920 x 1080 case is
    /// the point: the width's low two bits sit in dword 1 and the rest in dword 2, which a decode
    /// reading the width from one dword alone gets wrong; and the small case carries a base with a
    /// high byte, so the two halves of the address are both exercised.
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
            }
        );

        // 1920 x 1080, tiled 64KB_R_X (ADDR_SW_64KB_R_X = 27): the width split *and* the tiling read
        // from dword 3, so a decode that ignored word 3 would call this linear.
        assert_eq!(
            decode_image_descriptor(encode(0x2_0000_0000, 1920, 1080, 0x0C, 27)),
            ImageDescriptor {
                base: 0x2_0000_0000,
                width: 1920,
                height: 1080,
                format: 0x0C,
                tiling: SwizzleMode::Tiled64KbRX,
            }
        );
    }

    /// **A scissor decodes its rectangle from the GENERIC_SCISSOR corners.**
    ///
    /// The measured draw oracle writes top-left `0x80000000` (origin, plus the `WINDOW_OFFSET_DISABLE`
    /// flag at bit 31) and bottom-right `0x00400040` for a 64x64 frame. Made to fail against a decode
    /// that read the flag bit into `y` (it would give a huge height), or masked the wrong field. A
    /// sub-rect with a non-zero origin exercises the top-left, which the full-frame case cannot.
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

        // A sub-rect: top-left (x 16, y 8) is 0x0008_0010, bottom-right (x 48, y 56) is 0x0038_0030.
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

    /// **The scissor reads the live GENERIC_SCISSOR writes, and is absent when a corner is missing.**
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
