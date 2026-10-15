//! The user-data words and registers a computed access can depend on (D753): those its
//! descriptor or base and its offset are computed from, through the program before it.
//!
//! A replay of the prefix (`evaluate::scalar_prefix_explained`) answers the access's registers
//! from the user data, the system registers it is seeded with and the guest words it reads. Only
//! the registers those answers flow from matter: a backward slice from the access's own registers,
//! closed over every instruction that writes one, the branches that choose which do, and the
//! condition code where a write in the slice reads it. A draw whose other words differ - its own
//! constants, or tables the program loads and never reaches the access through - replays to the
//! same answer.
//!
//! The slice never lets a write end a register's liveness, so it holds for any path through the
//! prefix, loops included, at the cost of keeping a register a later write would have replaced.
//! A vector instruction that writes a register in the slice ends it: the slice falls back to every
//! register the prefix reads.

use orbistoun_shader::{Decode, EncodingTable, Instruction, Operand};

use crate::draw_buffers::{Computed, ComputedAccess};

/// The scalar registers that are general-purpose: `s0` to `s105`. The codes past them name `vcc`,
/// `m0`, `exec` and the constants, none of them user data.
const SCALAR_REGISTERS: u16 = 106;

/// The user-data words `computed` may read before or at its access, as a mask of the stage's words
/// (bit `i` for word `i`), with `prefix` its program before the access decoded with `encodings`.
///
/// A replay of the prefix answers the access's registers from no user-data word outside the mask,
/// so two draws whose words agree inside it, and whose seeded registers agree on
/// [`computed_registers`], replay alike. [`None`] where nothing here bounds the words read: an
/// instruction the tables do not name, a move relative to `m0` or a jump to a computed address,
/// or a stage with more words than the mask holds.
#[must_use]
pub fn computed_user_data(
    computed: &Computed,
    prefix: &Decode,
    encodings: &EncodingTable,
) -> Option<u64> {
    let first = u16::try_from(computed.first_register).ok()?;
    let count = u16::try_from(computed.user_data).ok()?;
    if count > 64 {
        return None;
    }
    let live = computed_registers(computed, prefix, encodings)?;
    Some(
        (0..count)
            .filter(|word| live >> (first + word) & 1 == 1)
            .fold(0, |mask, word| mask | 1 << word),
    )
}

/// The scalar registers, by code below 128 (`s0`-`s105`, `vcc`, `m0`, `exec`), whose values at the
/// program's entry the access's descriptor or base and offset may be computed from: bit `r` for
/// register `r`. A seeded register outside it does not change what a replay answers. [`None`] as
/// for [`computed_user_data`].
#[must_use]
pub fn computed_registers(
    computed: &Computed,
    prefix: &Decode,
    encodings: &EncodingTable,
) -> Option<u128> {
    let mut effects = Vec::with_capacity(prefix.instructions.len());
    for instruction in &prefix.instructions {
        let family = encodings
            .encodings()
            .get(usize::from(instruction.encoding?))?;
        let name = encodings.mnemonic_for(&family.name, instruction.opcode)?;
        effects.push(effect(instruction, &family.name, name)?);
    }
    let mut live = 0u128;
    let mut condition = false;
    // The access: its descriptor's four registers or its base pair, and an offset register.
    match computed.through {
        ComputedAccess::Descriptor { first, .. } => live |= span(first, 4),
        ComputedAccess::Pointer { base, .. } => live |= span(base, 2),
    }
    if u16::from(computed.soffset) < SCALAR_REGISTERS {
        live |= span(u16::from(computed.soffset), 1);
    }
    // Every branch decides which writes happen.
    for effect in effects.iter().filter(|effect| effect.kind == Kind::Branch) {
        live |= effect.reads;
        condition |= effect.reads_condition;
    }
    loop {
        let before = (live, condition);
        for effect in &effects {
            if effect.writes & live == 0 && !(effect.writes_condition && condition) {
                continue;
            }
            if effect.kind == Kind::Vector {
                return Some(effects.iter().fold(live, |all, effect| all | effect.reads));
            }
            live |= effect.reads;
            condition |= effect.reads_condition;
        }
        if (live, condition) == before {
            return Some(live);
        }
    }
}

/// What one instruction reads and writes among the scalar registers and the condition code.
struct Effect {
    reads: u128,
    writes: u128,
    reads_condition: bool,
    writes_condition: bool,
    kind: Kind,
}

/// What sort of instruction an [`Effect`] is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A scalar one, or one that writes no scalar.
    Scalar,
    /// A vector one, whose scalar writes - a compare's mask, a lane read, a carry out - come from
    /// lanes the slice does not follow.
    Vector,
    /// A branch on what it reads.
    Branch,
}

/// The registers from `first`, `width` of them, as bits; codes past 127 are not registers here.
fn span(first: u16, width: u16) -> u128 {
    (first..first.saturating_add(width))
        .filter(|&at| at < 128)
        .fold(0, |bits, at| bits | 1 << at)
}

/// The register a scalar operand names, by its code: a general register, `vcc`, `m0` or `exec`.
fn register(operand: &Operand) -> Option<u16> {
    match operand {
        Operand::Scalar(register) if *register < SCALAR_REGISTERS => Some(*register),
        Operand::Named(name) => match name.as_str() {
            "vcc_lo" | "vcc" => Some(106),
            "vcc_hi" => Some(107),
            "m0" => Some(124),
            "exec_lo" | "exec" => Some(126),
            "exec_hi" => Some(127),
            _ => None,
        },
        _ => None,
    }
}

/// Whether the first type in an instruction's name is 64 bits wide, as its destination's is
/// (`s_bfe_u64` and `s_cselect_b64` write a pair, `s_bcnt1_i32_b64` one register).
fn wide_destination(name: &str) -> bool {
    name.split('_')
        .find(|part| {
            matches!(part.as_bytes(), [b'b' | b'u' | b'i' | b'f', rest @ ..]
                if !rest.is_empty() && rest.iter().all(u8::is_ascii_digit))
        })
        .is_some_and(|kind| kind.ends_with("64"))
}

/// How many registers a scalar load fills, from the width its name states.
fn loaded_words(name: &str) -> u16 {
    match name.rsplit_once("dwordx") {
        Some((_, count)) => count.parse().unwrap_or(16),
        None => 1,
    }
}

/// What an instruction reads and writes; [`None`] for one whose registers its operands do not name:
/// a relative move, or a jump to an address in registers.
///
/// A destination is the first operand of the scalar ALU and load families, and of a vector
/// instruction that writes a scalar; the second of a vector instruction with a carry or scale out;
/// `exec` for a save-exec; and `vcc` for a short vector compare or carry. One that also reads its
/// destination - an add or multiply into a constant's register, a bit set - reads it too. Read
/// widths are taken generously: a pair for a 64-bit scalar operand and for every scalar a vector
/// instruction reads (a lane mask is a pair), four for a buffer descriptor, eight for an image's.
fn effect(instruction: &Instruction, family: &str, name: &str) -> Option<Effect> {
    if name.starts_with("s_movrel")
        || name.starts_with("v_movrel")
        || name.starts_with("s_setpc")
        || name.starts_with("s_swappc")
        || name.starts_with("s_rfe")
        || name.starts_with("s_cbranch_join")
    {
        return None;
    }
    let vector = matches!(family, "VOP1" | "VOP2" | "VOPC" | "VOP3" | "VOP3P");
    let carry = ["_co_", "div_scale", "mad_u64", "mad_i64"]
        .iter()
        .any(|carry| name.contains(carry));
    let destinations: &[usize] = match family {
        "SOP1" | "SOP2" => &[0],
        "SOPK" if !name.starts_with("s_cmpk") && !name.starts_with("s_setreg") => &[0],
        "SMEM"
            if name.starts_with("s_load")
                || name.starts_with("s_buffer_load")
                || name.starts_with("s_scratch_load")
                || name.starts_with("s_mem")
                || name.starts_with("s_get_waveid") =>
        {
            &[0]
        }
        // A short compare's operands are its sources: it writes `vcc`, which none names.
        "VOPC" => &[],
        _ if vector && carry => &[0, 1],
        _ if vector => &[0],
        _ => &[],
    };
    // A destination also read: an add or multiply into a register, a bit set.
    let reads_destination = name.starts_with("s_addk")
        || name.starts_with("s_mulk")
        || name.starts_with("s_cmovk")
        || name.starts_with("s_bitset");
    let mut effect = Effect {
        reads: 0,
        writes: 0,
        reads_condition: name.starts_with("s_cselect")
            || name.starts_with("s_cmov")
            || name.starts_with("s_addc")
            || name.starts_with("s_subb")
            || name.starts_with("s_cbranch_scc"),
        writes_condition: matches!(family, "SOP1" | "SOP2" | "SOPC" | "SOPK"),
        kind: if vector {
            Kind::Vector
        } else if name.starts_with("s_cbranch") {
            Kind::Branch
        } else {
            Kind::Scalar
        },
    };
    for (position, operand) in instruction.operands.iter().enumerate() {
        let Some(at) = register(operand) else {
            continue;
        };
        let (written, read) = widths(family, name, position);
        if destinations.contains(&position) {
            effect.writes |= span(at, written);
            if !reads_destination {
                continue;
            }
        }
        effect.reads |= span(at, read);
    }
    implied(&mut effect, instruction, (family, name, carry));
    Some(effect)
}

/// The registers an instruction reads or writes that no operand names: a scalar load's offset
/// register, a save-exec's `exec`, a short compare's or carry's `vcc`, and a branch's condition.
fn implied(
    effect: &mut Effect,
    instruction: &Instruction,
    (family, name, carry): (&str, &str, bool),
) {
    // A scalar load's offset register is not an operand: the second word's bits 31:25.
    if family == "SMEM"
        && let Some(second) = instruction.second_word
        && let Ok(code) = u16::try_from((second >> 25) & 0x7f)
        && code < SCALAR_REGISTERS
    {
        effect.reads |= span(code, 1);
    }
    if name.contains("saveexec") {
        effect.reads |= span(126, 2);
        effect.writes |= span(126, 2);
    }
    // A short compare writes its mask to `vcc`, and a short carry its carry.
    if family == "VOPC" || (family == "VOP2" && carry) {
        effect.writes |= span(106, 2);
    }
    if name.starts_with("s_cbranch_vcc") {
        effect.reads |= span(106, 2);
    }
    if name.starts_with("s_cbranch_exec") {
        effect.reads |= span(126, 2);
    }
}

/// How many registers an operand at `position` spans written and read: a scalar load's dwords, a
/// pair for a vector instruction's mask or carry and for a 64-bit scalar destination, one
/// otherwise; read, four for a buffer descriptor, eight for an image's, a pair for a load's base, a
/// flat base, any scalar a vector instruction reads and any operand of a 64-bit scalar one.
fn widths(family: &str, name: &str, position: usize) -> (u16, u16) {
    let vector = matches!(family, "VOP1" | "VOP2" | "VOPC" | "VOP3" | "VOP3P");
    let written = match family {
        "SMEM" => loaded_words(name),
        _ if vector && (position == 1 || name.starts_with("v_cmp")) => 2,
        _ if wide_destination(name) => 2,
        _ => 1,
    };
    let read = match family {
        "SMEM" if name.starts_with("s_buffer") => 4,
        "MUBUF" | "MTBUF" if position == 2 => 4,
        "MIMG" => 8,
        "SMEM" | "FLAT" => 2,
        _ if vector || name.contains("64") => 2,
        _ => 1,
    };
    (written, read)
}

#[cfg(test)]
mod tests {
    use super::computed_user_data;
    use crate::draw_buffers::{Computed, ComputedAccess, DescriptorReads};
    use orbistoun_shader::{EncodingTable, OperandTable, decode_program};

    /// A computed access's mask, for a prefix of words and a stage of `count` user-data words from
    /// `s4`, reading through the descriptor in `first` with the offset code `soffset`.
    fn mask(prefix: &[u32], count: u32, (first, soffset): (u16, u8)) -> Option<u64> {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = prefix.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode_program(&bytes, &encodings, &operands);
        assert!(decoded.is_trustworthy(), "the fixture decodes cleanly");
        let computed = Computed {
            prefix: prefix.to_vec(),
            at: u32::try_from(prefix.len() * 4).expect("small"),
            through: ComputedAccess::Descriptor {
                first,
                reads: DescriptorReads::default(),
            },
            soffset,
            first_register: 4,
            user_data: count,
            program_address: None,
        };
        computed_user_data(&computed, &decoded, &encodings)
    }

    /// `s_load_dwordx4 s[0:3], s[4:5], 0x0`: a descriptor loaded from a table at the stage's first
    /// two words.
    const LOAD_DESCRIPTOR: [u32; 2] = [0xf408_0002, 0xfa00_0000];
    /// `s_load_dwordx4 s[16:19], s[6:7], 0x0`: a table at the next two words, loaded and not read
    /// through.
    const LOAD_ELSEWHERE: [u32; 2] = [0xf408_0403, 0xfa00_0000];
    /// `s_waitcnt lgkmcnt(0)`.
    const WAIT: u32 = 0xbf8c_c07f;

    /// A descriptor loaded from a table the first two words name depends on those two alone: the
    /// stage's other words - a draw's own constants - do not change what the replay finds.
    #[test]
    fn only_the_words_the_access_is_computed_from_are_in_the_mask() {
        let mut prefix = LOAD_DESCRIPTOR.to_vec();
        prefix.push(WAIT);
        // Through s[0:3], which the load wrote, with an inline constant offset.
        assert_eq!(mask(&prefix, 12, (0, 128 + 16)), Some(0b11));
    }

    /// A table the program loads and never reads the access through is left out: Grand Theft Auto
    /// V's computed accesses load three tables from per-draw pointers in user data and read through
    /// one, so a mask of every word read changed on nearly every draw.
    #[test]
    fn a_table_that_does_not_reach_the_access_is_left_out() {
        let mut prefix = LOAD_ELSEWHERE.to_vec();
        prefix.extend(LOAD_DESCRIPTOR);
        prefix.push(WAIT);
        assert_eq!(mask(&prefix, 12, (0, 125)), Some(0b11));
        // Read through it, it counts.
        assert_eq!(mask(&prefix, 12, (16, 125)), Some(0b1100));
    }

    /// The access's own registers count: a descriptor still in user data, and an offset register.
    /// The load before it writes nothing they read, so its table is left out.
    #[test]
    fn the_access_s_descriptor_and_offset_registers_count() {
        let mut prefix = LOAD_DESCRIPTOR.to_vec();
        prefix.push(WAIT);
        // Through s[8:11] (words 4-7), the offset in s14 (word 10).
        assert_eq!(mask(&prefix, 12, (8, 14)), Some(0b1111 << 4 | 1 << 10));
    }

    /// A write passes its reads back: `s_mov_b32 s4, s13` puts word 9 behind the load's base, and
    /// `s_mov_b32 s6, 0` reads nothing. A register written is still kept, since another path could
    /// reach the load without the write.
    #[test]
    fn a_write_passes_on_what_it_reads() {
        let mut prefix = vec![0xbe86_0380, 0xbe84_030d];
        prefix.extend(LOAD_DESCRIPTOR);
        prefix.push(WAIT);
        assert_eq!(mask(&prefix, 12, (0, 125)), Some(0b11 | 1 << 9));
    }

    /// An add into a register reads it: `s_addk_i32 s0, 1` after `s_mov_b32 s0, s13` keeps word 9
    /// behind the descriptor's first register.
    #[test]
    fn an_add_into_a_register_reads_it() {
        assert_eq!(
            mask(&[0xbe80_030d, 0xb780_0001], 12, (0, 125)),
            Some(1 << 9)
        );
    }

    /// An instruction the tables do not name may write anything: `s_cmov_b32 s6, s7`.
    #[test]
    fn an_unnamed_instruction_bounds_nothing() {
        assert_eq!(mask(&[0xbe86_0507], 12, (0, 125)), None);
    }

    /// A scalar load's offset register, which no operand names, counts: `s_load_dwordx4 s[0:3],
    /// s[4:5], s15` reads word 11.
    #[test]
    fn a_scalar_load_s_offset_register_counts() {
        assert_eq!(
            mask(&[0xf408_0002, 0x1e00_0000], 12, (0, 125)),
            Some(0b11 | 1 << 11)
        );
    }
}
