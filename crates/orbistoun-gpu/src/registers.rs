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
                        value: *value,
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
                let Some((offset, values)) = words.split_first() else {
                    continue;
                };
                let offset = &(offset & 0xffff);
                for (index, value) in values.iter().enumerate() {
                    writes.push(RegisterWrite {
                        packet_offset: packet.offset,
                        register: base
                            .wrapping_add(*offset)
                            .wrapping_add(u32::try_from(index).unwrap_or(0)),
                        value: *value,
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
                    instances = *count;
                }
            }
            DRAW_INDEX_AUTO => {
                if let Some(vertices) = words.first() {
                    draws.push(DrawCall {
                        packet_offset: packet.offset,
                        instances,
                        kind: DrawKind::Auto {
                            vertices: *vertices,
                        },
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
                            indices: *indices,
                            address: u64::from(*lo) | (u64::from(*hi) << 32),
                        },
                    });
                }
            }
            _ => {}
        }
    }
    draws
}

fn read_words(body: &[u8], start: usize, length: usize) -> Option<Vec<u32>> {
    let end = start.checked_add(length)?;
    let slice = body.get(start..end)?;
    Some(
        slice
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
    )
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
                groups: [*x, *y, *z],
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
    use super::{
        BufferDescriptor, ColourTarget, ColourTargetExtent, DispatchCall, DrawKind, DrawOrDispatch,
        ImageDescriptor, RegisterWrite, Scissor, SwizzleMode, Vocabulary, buffer_descriptor_at,
        colour_swizzle_mode_at, colour_target_at, colour_target_extent_at, correlate_draws,
        decode_buffer_descriptor, decode_colour_swizzle_mode, decode_colour_target_extent,
        decode_image_descriptor, decode_scissor, decode_target_mask, dispatch_calls, draw_calls,
        register_writes, scissor_at, shader_candidates, target_mask_at,
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
