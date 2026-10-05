//! The buffers a draw's shader reads through, traced to where a draw finds them (D733).
//!
//! A shader names a buffer by a four-word descriptor in scalar registers, or reads straight from a
//! 64-bit base with a scalar load. Each is traced back through the program to what the draw
//! supplies - its user-data words, constants, and words a scalar load read from a table at a base
//! the draw supplies - so a caller can resolve it per draw and bind the guest range it names. What
//! cannot be traced is left to the window.

use std::collections::BTreeMap;

use orbistoun_shader::{Decode, EncodingTable, Instruction, Operand};
use serde::{Deserialize, Serialize};

use crate::wavefront::{TableBase, TableWord};

/// One word of a buffer descriptor, as the program formed it before reading through it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DescriptorWord {
    /// The stage's user-data word at this index.
    UserData(u32),
    /// This value.
    Constant(u32),
    /// The word a scalar load read at `offset` bytes past `table`.
    Loaded {
        /// Where the table is.
        table: TableBase,
        /// Byte offset of the word in it.
        offset: u32,
    },
}

/// Where one of a module's buffers comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum BufferSource {
    /// A buffer resource constant: its four words, and how the program reads through it. The range
    /// is the one the descriptor's bounds rules admit for those reads.
    Descriptor {
        /// The descriptor's words, in register order.
        words: [DescriptorWord; 4],
        /// What the accesses through it reach.
        reads: DescriptorReads,
    },
    /// A scalar load's base, and how many bytes past it the program's loads through it reach.
    Pointer {
        /// The base address.
        base: TableBase,
        /// One past the last byte any load through the base reads.
        extent: u32,
    },
}

/// How a program's accesses through one descriptor reach into its buffer: what bounds the bytes a
/// draw must bind for them, beside the descriptor's own fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DescriptorReads {
    /// One past the last byte any scalar buffer load through it reads, from the buffer's start;
    /// zero when none does.
    pub scalar_reach: u32,
    /// Whether a vector access reads through it, so its out-of-bounds mode applies.
    pub vector: bool,
    /// One past the last byte any vector access reads from its record's start - its immediate
    /// offset and its width - or [`None`] when one adds a register offset, which nothing bounds
    /// here.
    pub vector_reach: Option<u32>,
    /// Whether a vector access indexes records.
    pub indexed: bool,
}

impl DescriptorReads {
    /// These reads together with `other`'s.
    #[must_use]
    pub fn and(self, other: Self) -> Self {
        Self {
            scalar_reach: self.scalar_reach.max(other.scalar_reach),
            vector: self.vector || other.vector,
            vector_reach: self
                .vector_reach
                .zip(other.vector_reach)
                .map(|(a, b)| a.max(b)),
            indexed: self.indexed || other.indexed,
        }
    }
}

impl Default for DescriptorReads {
    fn default() -> Self {
        Self {
            scalar_reach: 0,
            vector: false,
            vector_reach: Some(0),
            indexed: false,
        }
    }
}

/// A module's buffers and the accesses that read through each.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DrawBuffers {
    /// Each buffer, by slot, in the order the program first reaches it.
    pub sources: Vec<BufferSource>,
    /// The slot each traced access reads through, by the access's byte offset in the program.
    pub served: BTreeMap<u32, u32>,
}

/// Traces the buffers a program reads through, for a stage whose `count` user-data words land in
/// the scalar registers from `first_register`.
///
/// Nothing is traced in a program that writes memory, which a snapshot bound for the draw would
/// not see.
#[must_use]
pub fn trace(
    decode: &Decode,
    encodings: &EncodingTable,
    first_register: u32,
    count: u32,
) -> DrawBuffers {
    let named: Vec<Option<(&str, &str)>> = decode
        .instructions
        .iter()
        .map(|instruction| {
            let family = encodings
                .encodings()
                .get(usize::from(instruction.encoding?))?;
            Some((
                family.name.as_str(),
                encodings.mnemonic_for(&family.name, instruction.opcode)?,
            ))
        })
        .collect();
    if named.iter().flatten().any(|&(_, name)| writes_memory(name)) {
        return DrawBuffers::default();
    }
    let Ok(blocks) = crate::blocks::split(decode, |instruction| {
        let family = encodings
            .encodings()
            .get(usize::from(instruction.encoding?))?;
        Some(family.name.clone())
    }) else {
        return DrawBuffers::default();
    };
    let entry = entry_state(first_register, count);
    let reached = block_entries(decode, &named, &blocks, entry);

    // Every access, with the state it reads, in program order: the bases' reach is known only once
    // every load through them has been seen.
    let mut accesses: Vec<(u32, Access)> = Vec::new();
    for (block, state) in blocks.iter().zip(reached) {
        let Some(mut held) = state else {
            continue;
        };
        for (instruction, named) in decode.instructions[block.first..block.end]
            .iter()
            .zip(&named[block.first..block.end])
        {
            if let Some(&(_, name)) = named.as_ref()
                && let Some(access) = access(instruction, name, &held)
            {
                accesses.push((instruction.offset, access));
            }
            step(&mut held, instruction, *named);
        }
    }
    accesses.sort_by_key(|(offset, _)| *offset);
    slots(accesses)
}

/// Gives each distinct descriptor or base a slot, in the order the program first reaches it, with
/// every access's reach through it combined.
fn slots(accesses: Vec<(u32, Access)>) -> DrawBuffers {
    let mut keys: Vec<Key> = Vec::new();
    let mut sources: Vec<BufferSource> = Vec::new();
    let mut buffers = DrawBuffers::default();
    for (offset, access) in accesses {
        let (key, source) = match access {
            Access::Descriptor(words, reads) => (
                Key::Descriptor(words),
                BufferSource::Descriptor { words, reads },
            ),
            Access::Pointer { base, end } => (
                Key::Pointer(base),
                BufferSource::Pointer { base, extent: end },
            ),
        };
        let slot = if let Some(slot) = keys.iter().position(|k| *k == key) {
            sources[slot] = match (sources[slot], source) {
                (
                    BufferSource::Descriptor { words, reads },
                    BufferSource::Descriptor { reads: more, .. },
                ) => BufferSource::Descriptor {
                    words,
                    reads: reads.and(more),
                },
                (
                    BufferSource::Pointer { base, extent },
                    BufferSource::Pointer { extent: more, .. },
                ) => BufferSource::Pointer {
                    base,
                    extent: extent.max(more),
                },
                (kept, _) => kept,
            };
            slot
        } else {
            keys.push(key);
            sources.push(source);
            sources.len() - 1
        };
        buffers
            .served
            .insert(offset, u32::try_from(slot).unwrap_or(u32::MAX));
    }
    buffers.sources = sources;
    buffers
}

/// What makes two accesses read one buffer: the same descriptor words, or the same base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Descriptor([DescriptorWord; 4]),
    Pointer(TableBase),
}

/// What each scalar register holds, where the program's own writes fix it: [`None`] where it could
/// hold more than one thing.
type Held = Vec<Option<DescriptorWord>>;

/// One traced access, before its base's reach is known.
#[derive(Debug, Clone, Copy)]
enum Access {
    /// Through a buffer descriptor, reaching as far as the reads say.
    Descriptor([DescriptorWord; 4], DescriptorReads),
    /// A scalar load at a base, reaching `end` bytes past it.
    Pointer { base: TableBase, end: u32 },
}

/// The registers at entry: each user-data word where the hardware loads it, and nothing else known.
fn entry_state(first_register: u32, count: u32) -> Held {
    let mut held: Held = vec![None; crate::model::SCALAR_REGISTERS as usize];
    for index in 0..count {
        if let Some(word) = held.get_mut((first_register + index) as usize) {
            *word = Some(DescriptorWord::UserData(index));
        }
    }
    held
}

/// What each reachable block starts with, found by carrying states along every branch until they
/// settle; a register two paths disagree on is unknown. [`None`] for a block nothing reaches.
fn block_entries(
    decode: &Decode,
    named: &[Option<(&str, &str)>],
    blocks: &[crate::blocks::Block],
    entry: Held,
) -> Vec<Option<Held>> {
    use crate::blocks::{Terminator, block_at};
    let mut entries: Vec<Option<Held>> = vec![None; blocks.len()];
    let Some(first) = entries.first_mut() else {
        return entries;
    };
    *first = Some(entry);
    let mut pending = vec![0usize];
    while let Some(index) = pending.pop() {
        let (Some(block), Some(Some(start))) = (blocks.get(index), entries.get(index)) else {
            continue;
        };
        let mut held = start.clone();
        for (instruction, named) in decode.instructions[block.first..block.end]
            .iter()
            .zip(&named[block.first..block.end])
        {
            step(&mut held, instruction, *named);
        }
        let next: Vec<u32> = match block.terminator {
            Terminator::End => Vec::new(),
            Terminator::Jump { target } => vec![target],
            Terminator::Branch {
                target,
                fallthrough,
            } => vec![target, fallthrough],
            Terminator::Fallthrough { next } => vec![next],
        };
        for successor in next.into_iter().filter_map(|at| block_at(blocks, at)) {
            let changed = match &mut entries[successor] {
                slot @ None => {
                    *slot = Some(held.clone());
                    true
                }
                Some(known) => meet(known, &held),
            };
            if changed {
                pending.push(successor);
            }
        }
    }
    entries
}

/// Narrows `known` to what it shares with `other`, answering whether anything changed.
fn meet(known: &mut Held, other: &Held) -> bool {
    let mut changed = false;
    for (word, theirs) in known.iter_mut().zip(other) {
        if word.is_some() && *word != *theirs {
            *word = None;
            changed = true;
        }
    }
    changed
}

/// Whether an instruction writes memory a draw's bound buffers snapshot. The local data share is
/// the wavefront's own.
fn writes_memory(name: &str) -> bool {
    (name.contains("store") || name.contains("atomic")) && !name.starts_with("ds_")
}

/// `null` in a scalar memory instruction's `SOFFSET` field, bits 31:25 of its second word: no
/// offset register.
const SOFFSET_NULL: u32 = 125;

/// Whether a scalar memory access adds nothing to its immediate: no offset register, and no offset
/// bits above the sixteen the operand layout gives. Any other is left untraced.
fn plain_scalar_offset(instruction: &Instruction) -> bool {
    let second = instruction.second_word.unwrap_or(0);
    (second >> 25) & 0x7f == SOFFSET_NULL && (second >> 16).trailing_zeros() >= 5
}

/// How many registers a scalar memory access fills, from the width its name states.
fn scalar_words(name: &str) -> u32 {
    match name.rsplit_once("dwordx") {
        Some((_, count)) => count.parse().unwrap_or(16),
        None => 1,
    }
}

/// The four words of the descriptor in `first..first + 4`, when all four are traced.
fn descriptor(held: &Held, first: u16) -> Option<[DescriptorWord; 4]> {
    let at = |step: u16| held.get(usize::from(first + step)).copied().flatten();
    Some([at(0)?, at(1)?, at(2)?, at(3)?])
}

/// The base a scalar load reads from, when both halves are the draw's words or constants.
fn table_base(held: &Held, first: u16) -> Option<TableBase> {
    let half = |step: u16| match held.get(usize::from(first + step)).copied().flatten()? {
        DescriptorWord::UserData(index) => Some(TableWord::UserData(index)),
        DescriptorWord::Constant(value) => Some(TableWord::Constant(value)),
        DescriptorWord::Loaded { .. } => None,
    };
    Some(TableBase {
        low: half(0)?,
        high: half(1)?,
    })
}

/// The traced access an instruction makes, if it reads through a buffer and its descriptor or base
/// is traced.
fn access(instruction: &Instruction, name: &str, held: &Held) -> Option<Access> {
    let operands = instruction.operands.as_slice();
    if name.starts_with("s_load_dword") || name.starts_with("s_buffer_load_dword") {
        let [_, Operand::Scalar(base), Operand::Immediate(offset)] = operands else {
            return None;
        };
        if !plain_scalar_offset(instruction) {
            return None;
        }
        let end = u32::try_from(*offset)
            .ok()?
            .checked_add(scalar_words(name) * 4)?;
        if name.starts_with("s_buffer_load") {
            let reads = DescriptorReads {
                scalar_reach: end,
                ..DescriptorReads::default()
            };
            return Some(Access::Descriptor(descriptor(held, *base)?, reads));
        }
        return Some(Access::Pointer {
            base: table_base(held, *base)?,
            end,
        });
    }
    if name.starts_with("buffer_") || name.starts_with("tbuffer_") {
        // The scalar offset is added to the address and not to the range check, so only an access
        // that adds none is bounded by what the descriptor says.
        let [_, _, Operand::Scalar(first), soffset] = operands else {
            return None;
        };
        if !matches!(soffset, Operand::Integer(0)) && *soffset != Operand::Named("null".into()) {
            return None;
        }
        // The addressing modifiers: the immediate offset in bits 11:0, `offen` at 12, `idxen` at 13.
        let word = instruction.word;
        let reach =
            (word & (1 << 12) == 0).then(|| (word & 0xfff) + vector_payload(instruction, name));
        let reads = DescriptorReads {
            scalar_reach: 0,
            vector: true,
            vector_reach: reach,
            indexed: word & (1 << 13) != 0,
        };
        return Some(Access::Descriptor(descriptor(held, *first)?, reads));
    }
    None
}

/// How many bytes a vector buffer access reads or writes at its address: its dwords for an
/// untyped one, its element for a typed one, and sixteen - the widest - where neither is known.
fn vector_payload(instruction: &Instruction, name: &str) -> u32 {
    const WIDEST: u32 = 16;
    if name.starts_with("tbuffer_") {
        static FORMATS: std::sync::OnceLock<orbistoun_shader::FormatTable> =
            std::sync::OnceLock::new();
        let formats =
            FORMATS.get_or_init(|| orbistoun_shader::FormatTable::builtin().unwrap_or_default());
        return formats
            .get(orbistoun_shader::FormatTable::field(instruction.word))
            .map_or(WIDEST, |format| {
                format.widths.iter().sum::<u32>().div_ceil(8)
            });
    }
    match name.rsplit_once("dword") {
        Some((_, "")) => 4,
        Some((_, count)) => count
            .strip_prefix('x')
            .and_then(|n| n.parse::<u32>().ok())
            .map_or(WIDEST, |n| n * 4),
        None => WIDEST,
    }
}

/// Forgets `count` registers from `first`.
fn forget(held: &mut Held, first: u16, count: u32) {
    for word in held
        .iter_mut()
        .skip(usize::from(first))
        .take(count as usize)
    {
        *word = None;
    }
}

/// Carries `held` over one instruction.
///
/// A move from a constant or a traced register, and a scalar load from a traced base, are followed.
/// Every other scalar register an instruction can write is forgotten, as wide as the widest
/// operand it can be: the destination of the scalar families; the destination of the vector ALU
/// families, which is a scalar one for a compare or a lane read; and the second operand of the
/// carry-out, divide-scale and 64-bit multiply-add forms, which write a scalar there. An
/// instruction with no name, or an indirect move, leaves nothing known.
fn step(held: &mut Held, instruction: &Instruction, named: Option<(&str, &str)>) {
    let Some((family, name)) = named else {
        held.fill(None);
        return;
    };
    let operands = instruction.operands.as_slice();
    if name.starts_with("s_movrel") {
        held.fill(None);
        return;
    }
    match (name, operands) {
        ("s_mov_b32", [Operand::Scalar(destination), source]) => {
            let value = match source {
                Operand::Scalar(from) => held.get(usize::from(*from)).copied().flatten(),
                Operand::Integer(value) => i32::try_from(*value)
                    .ok()
                    .map(|v| DescriptorWord::Constant(u32::from_ne_bytes(v.to_ne_bytes()))),
                Operand::Literal(value) => Some(DescriptorWord::Constant(*value)),
                _ => None,
            };
            if let Some(slot) = held.get_mut(usize::from(*destination)) {
                *slot = value;
            }
        }
        // The compact move of a sixteen-bit immediate, sign-extended, as ACO writes a small
        // constant such as a built descriptor's size.
        ("s_movk_i32", [Operand::Scalar(destination), Operand::Immediate(raw)]) => {
            let value = u16::try_from(*raw).ok().map(|raw| {
                DescriptorWord::Constant(u32::from_ne_bytes(
                    i32::from(i16::from_ne_bytes(raw.to_ne_bytes())).to_ne_bytes(),
                ))
            });
            if let Some(slot) = held.get_mut(usize::from(*destination)) {
                *slot = value;
            }
        }
        ("s_mov_b64", [Operand::Scalar(destination), Operand::Scalar(source)]) => {
            let pair = [
                held.get(usize::from(*source)).copied().flatten(),
                held.get(usize::from(source + 1)).copied().flatten(),
            ];
            for (step, value) in (0..).zip(pair) {
                if let Some(slot) = held.get_mut(usize::from(destination + step)) {
                    *slot = value;
                }
            }
        }
        (
            _,
            [
                Operand::Scalar(destination),
                Operand::Scalar(base),
                Operand::Immediate(offset),
            ],
        ) if name.starts_with("s_load_dword") => {
            let words = scalar_words(name);
            let table = table_base(held, *base).filter(|_| plain_scalar_offset(instruction));
            let offset = u32::try_from(*offset).ok();
            forget(held, *destination, words);
            if let (Some(table), Some(offset)) = (table, offset) {
                for step in 0..words {
                    if let Some(slot) = held.get_mut(usize::from(*destination) + step as usize) {
                        *slot = Some(DescriptorWord::Loaded {
                            table,
                            offset: offset + step * 4,
                        });
                    }
                }
            }
        }
        _ => {
            let (first, second) = match family {
                "SOP1" | "SOP2" | "SOPK" => (true, false),
                "SMEM" => {
                    if let Some(Operand::Scalar(destination)) = operands.first() {
                        forget(held, *destination, scalar_words(name).max(4));
                    }
                    (false, false)
                }
                "VOP1" | "VOP2" | "VOPC" => (true, true),
                "VOP3" => (
                    true,
                    name.contains("_co_")
                        || name.contains("div_scale")
                        || name.contains("mad_u64")
                        || name.contains("mad_i64"),
                ),
                _ => (false, false),
            };
            for (position, writes) in [first, second].into_iter().enumerate() {
                if let (true, Some(Operand::Scalar(register))) = (writes, operands.get(position)) {
                    forget(held, *register, 2);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BufferSource, DescriptorReads, DescriptorWord, trace};
    use crate::wavefront::{TableBase, TableWord};
    use orbistoun_shader::{EncodingTable, OperandTable, decode_program};

    /// Traces a program of words, for a stage whose user data starts at `s0`.
    fn traced(words: &[u32], count: u32) -> super::DrawBuffers {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode_program(&bytes, &encodings, &operands);
        assert!(decoded.is_trustworthy(), "the fixture decodes cleanly");
        trace(&decoded, &encodings, 0, count)
    }

    const END: u32 = 0xbf81_0000;
    /// `s_waitcnt lgkmcnt(0)`.
    const WAIT: u32 = 0xbf8c_c07f;

    /// radeonsi's clear pixel shader: a raw descriptor built from user-data word 2 and three
    /// constants (`si_nir_lower_resource.c:27-51`), read whole by one scalar buffer load.
    const CLEAR: [u32; 10] = [
        0xbe80_0302, // s_mov_b32 s0, s2
        0xbe81_0384, // s_mov_b32 s1, 4
        0xbe82_0390, // s_mov_b32 s2, 16
        0xbe83_03ff, // s_mov_b32 s3, 0x31016fac
        0x3101_6fac,
        0xf428_0000, // s_buffer_load_dwordx4 s[0:3], s[0:3], 0x0
        0xfa00_0000,
        WAIT,
        0x7e00_0200, // v_mov_b32 v0, s0
        END,
    ];

    /// The clear shader's descriptor is traced to user-data word 2 and its three constants, and its
    /// one load reads through it.
    #[test]
    fn the_clear_shaders_descriptor_is_its_user_data_and_constants() {
        let buffers = traced(&CLEAR, 4);
        assert_eq!(
            buffers.sources,
            [BufferSource::Descriptor {
                words: [
                    DescriptorWord::UserData(2),
                    DescriptorWord::Constant(4),
                    DescriptorWord::Constant(16),
                    DescriptorWord::Constant(0x3101_6fac),
                ],
                reads: DescriptorReads {
                    scalar_reach: 16,
                    ..DescriptorReads::default()
                },
            }]
        );
        assert_eq!(buffers.served.get(&0x14), Some(&0));
        assert_eq!(buffers.served.len(), 1);
    }

    /// A descriptor word moved in with `s_movk_i32` is its sixteen-bit immediate sign-extended, as
    /// for the clear shader's `s_mov_b32`.
    #[test]
    fn a_compact_move_is_a_sign_extended_constant() {
        let mut program = CLEAR;
        program[1] = 0xb001_8000; // s_movk_i32 s1, 0x8000
        program[2] = 0xb002_0100; // s_movk_i32 s2, 0x100
        let buffers = traced(&program, 4);
        let [BufferSource::Descriptor { words, .. }] = buffers.sources.as_slice() else {
            panic!("one traced descriptor: {:?}", buffers.sources);
        };
        assert_eq!(
            words[..3],
            [
                DescriptorWord::UserData(2),
                DescriptorWord::Constant(0xffff_8000),
                DescriptorWord::Constant(0x100),
            ]
        );
    }

    /// A descriptor loaded from a table is traced to the table's words, and the load that read it
    /// reads through the table's own buffer, which reaches as far as that load does.
    #[test]
    fn a_descriptor_from_a_table_is_the_tables_words() {
        let program = [
            0xf408_0100, // s_load_dwordx4 s[4:7], s[0:1], 0x10
            0xfa00_0010,
            WAIT,
            0xf420_0202, // s_buffer_load_dword s8, s[4:7], 0x0
            0xfa00_0000,
            END,
        ];
        let buffers = traced(&program, 2);
        let table = TableBase {
            low: TableWord::UserData(0),
            high: TableWord::UserData(1),
        };
        let loaded = |offset| DescriptorWord::Loaded { table, offset };
        assert_eq!(
            buffers.sources,
            [
                BufferSource::Pointer {
                    base: table,
                    extent: 0x20
                },
                BufferSource::Descriptor {
                    words: [loaded(0x10), loaded(0x14), loaded(0x18), loaded(0x1c)],
                    reads: DescriptorReads {
                        scalar_reach: 4,
                        ..DescriptorReads::default()
                    },
                },
            ]
        );
        assert_eq!(buffers.served.get(&0x0), Some(&0));
        assert_eq!(buffers.served.get(&0xc), Some(&1));
    }

    /// A register written with something else on one path to the load leaves it untraced: the
    /// value at the load is not one thing.
    #[test]
    fn a_register_written_on_one_path_is_not_traced() {
        let program = [
            0xbe80_0302, // s_mov_b32 s0, s2
            0xbe81_0384, // s_mov_b32 s1, 4
            0xbe82_0390, // s_mov_b32 s2, 16
            0xbe83_03ff, // s_mov_b32 s3, 0x31016fac
            0x3101_6fac,
            0xbf85_0001, // s_cbranch_scc0 +1
            0xbe80_0380, // s_mov_b32 s0, 0
            0xf428_0000, // s_buffer_load_dwordx4 s[0:3], s[0:3], 0x0
            0xfa00_0000,
            END,
        ];
        let buffers = traced(&program, 4);
        assert!(buffers.sources.is_empty(), "{:?}", buffers.sources);
        assert!(buffers.served.is_empty());
    }

    /// Paths that agree on a register keep it traced: a branch around an unrelated write joins with
    /// the descriptor intact.
    #[test]
    fn a_register_both_paths_agree_on_stays_traced() {
        let program = [
            0xbe80_0302, // s_mov_b32 s0, s2
            0xbe81_0384, // s_mov_b32 s1, 4
            0xbe82_0390, // s_mov_b32 s2, 16
            0xbe83_03ff, // s_mov_b32 s3, 0x31016fac
            0x3101_6fac,
            0xbf85_0001, // s_cbranch_scc1 +1
            0xbe8a_0380, // s_mov_b32 s10, 0
            0xf428_0000, // s_buffer_load_dwordx4 s[0:3], s[0:3], 0x0
            0xfa00_0000,
            END,
        ];
        assert_eq!(traced(&program, 4).sources.len(), 1);
    }

    /// A user-data register an arithmetic instruction overwrote is not the draw's word any more.
    #[test]
    fn an_overwritten_user_data_register_is_not_traced() {
        let program = [
            0x8102_ff02, // s_add_i32 s2, s2, 0x40
            0x0000_0040,
            0xbe80_0302, // s_mov_b32 s0, s2
            0xbe81_0384, // s_mov_b32 s1, 4
            0xbe82_0390, // s_mov_b32 s2, 16
            0xbe83_03ff, // s_mov_b32 s3, 0x31016fac
            0x3101_6fac,
            0xf428_0000, // s_buffer_load_dwordx4 s[0:3], s[0:3], 0x0
            0xfa00_0000,
            END,
        ];
        assert!(traced(&program, 4).sources.is_empty());
    }

    /// A program that writes memory is traced not at all: a buffer bound for the draw is its
    /// contents at the draw's start, and a store could change what a later load reads.
    #[test]
    fn a_program_that_stores_is_not_traced() {
        let mut program = CLEAR[..8].to_vec();
        // global_store_dword v[0:1], v2, off
        program.extend([0xdc70_8000, 0x007d_0200, END]);
        assert!(traced(&program, 4).sources.is_empty());
    }

    /// The clear shader's pixel module, translated for a draw that binds its buffers, declares
    /// them at the pixel stage's binding of the draw-buffer set and reads them there; translated
    /// for one that does not, it declares nothing there and reads the window as before.
    #[test]
    fn a_draw_that_binds_buffers_reads_the_clear_colour_through_its_binding() {
        use crate::wavefront::{MeshPrimitive, Stage, UserData, Window};
        use crate::{Fidelity, Strategy, Width, translate_with_user_data};
        use orbistoun_spirv::{DRAW_BUFFERS_SET, PIXEL_BUFFERS_BINDING, decoration, op};

        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = CLEAR.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode_program(&bytes, &encodings, &operands);
        let module = |draw_buffers: bool| {
            translate_with_user_data(
                &decoded,
                &encodings,
                Strategy::Predicated {
                    fidelity: Fidelity::Wavefront,
                    width: Width::Wave64,
                },
                (Stage::Fragment, MeshPrimitive::default()),
                Window::default(),
                UserData {
                    count: 4,
                    block_offset: 16,
                    draw_buffers,
                    ..UserData::default()
                },
            )
            .expect("the clear shader translates")
            .module
        };
        // Every instruction of a module, as its opcode and operand words.
        let instructions = |words: &[u32]| -> Vec<(u16, Vec<u32>)> {
            let mut at = 5;
            let mut found = Vec::new();
            while let Some(&word) = words.get(at) {
                let count = (word >> 16) as usize;
                found.push(((word & 0xffff) as u16, words[at + 1..at + count].to_vec()));
                at += count;
            }
            found
        };
        let decorated = |words: &[u32], what: u32, value: u32| {
            instructions(words).iter().any(|(opcode, operands)| {
                *opcode == op::DECORATE && operands.get(1..) == Some(&[what, value][..])
            })
        };
        let lengths = |words: &[u32]| {
            instructions(words)
                .iter()
                .filter(|(opcode, _)| *opcode == op::ARRAY_LENGTH)
                .count()
        };
        let bound = module(true);
        assert!(decorated(
            &bound,
            decoration::DESCRIPTOR_SET,
            DRAW_BUFFERS_SET
        ));
        assert!(decorated(
            &bound,
            decoration::BINDING,
            PIXEL_BUFFERS_BINDING
        ));
        assert_eq!(lengths(&bound), 4, "one read per word of the colour");
        let windowed = module(false);
        assert!(!decorated(
            &windowed,
            decoration::DESCRIPTOR_SET,
            DRAW_BUFFERS_SET
        ));
        assert_eq!(lengths(&windowed), 0);
    }

    /// Two loads through one descriptor read one buffer.
    #[test]
    fn two_loads_through_one_descriptor_share_a_slot() {
        let program = [
            0xbe80_0302, // s_mov_b32 s0, s2
            0xbe81_0384, // s_mov_b32 s1, 4
            0xbe82_0390, // s_mov_b32 s2, 16
            0xbe83_03ff, // s_mov_b32 s3, 0x31016fac
            0x3101_6fac,
            0xf420_0200, // s_buffer_load_dword s8, s[0:3], 0x0
            0xfa00_0000,
            0xf420_0240, // s_buffer_load_dword s9, s[0:3], 0x4
            0xfa00_0004,
            END,
        ];
        let buffers = traced(&program, 4);
        assert_eq!(buffers.sources.len(), 1);
        assert_eq!(buffers.served.values().copied().collect::<Vec<_>>(), [0, 0]);
        let BufferSource::Descriptor { reads, .. } = buffers.sources[0] else {
            panic!("a descriptor: {:?}", buffers.sources[0]);
        };
        assert_eq!(reads.scalar_reach, 8, "the second load reaches furthest");
    }

    /// The descriptor a vertex fetch indexes, in user data: an indexed access at an immediate
    /// offset reaches that offset and its width into each record.
    #[test]
    fn an_indexed_fetch_reaches_its_offset_and_width_into_a_record() {
        // buffer_load_dword v1, v0, s[0:3], 0 idxen offset:8
        let program = [0xe030_2008, 0x8000_0100, END];
        let buffers = traced(&program, 4);
        assert_eq!(
            buffers.sources,
            [BufferSource::Descriptor {
                words: [0, 1, 2, 3].map(DescriptorWord::UserData),
                reads: DescriptorReads {
                    scalar_reach: 0,
                    vector: true,
                    vector_reach: Some(12),
                    indexed: true,
                },
            }]
        );
    }

    /// A fetch that adds a scalar offset register is left to the window: the range check does not
    /// see that offset, so the descriptor does not bound where it reads.
    #[test]
    fn a_fetch_adding_a_scalar_offset_is_not_traced() {
        // buffer_load_dword v1, v0, s[0:3], s5 idxen offset:8
        let program = [0xe030_2008, 0x0500_0100, END];
        assert!(traced(&program, 6).sources.is_empty());
    }
}
