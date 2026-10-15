//! The user-data words a computed access can depend on (D753): every one the program before it
//! may read, and the ones the access itself reads its descriptor or base and its offset from.
//!
//! A replay of the prefix (`evaluate::scalar_prefix_explained`) is a function of these words, the
//! system registers it is seeded with and the guest words it reads, so a draw whose other user-data
//! words differ - its own constants, say - replays to the same answer. The set is found from the
//! instructions alone, without following paths: a register any instruction reads counts, whether
//! or not something wrote it first, so it holds every word a replay reads and may hold more.

use orbistoun_shader::{Decode, EncodingTable, Instruction, Operand};

use crate::draw_buffers::{Computed, ComputedAccess};

/// The scalar registers that are general-purpose: `s0` to `s105`. The codes past them name `vcc`,
/// `m0`, `exec` and the constants, none of them user data.
const SCALAR_REGISTERS: u16 = 106;

/// The user-data words `computed` may read before or at its access, as a mask of the stage's words
/// (bit `i` for word `i`), with `prefix` its program before the access decoded with `encodings`.
///
/// A replay of the prefix reads no user-data word outside the mask, so two draws whose words
/// agree inside it replay alike. [`None`] where nothing here bounds the words read: an
/// instruction the tables do not name, a move relative to `m0`, or a stage with more words than
/// the mask holds.
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
    let mut mask = 0u64;
    let mut read = |register: u16, width: u16| {
        for at in register..register.saturating_add(width) {
            if at >= first && at - first < count {
                mask |= 1 << (at - first);
            }
        }
    };
    for instruction in &prefix.instructions {
        let family = encodings
            .encodings()
            .get(usize::from(instruction.encoding?))?;
        let name = encodings.mnemonic_for(&family.name, instruction.opcode)?;
        for (register, width) in scalar_reads(instruction, &family.name, name)? {
            read(register, width);
        }
    }
    // The access: its descriptor's four registers or its base pair, and an offset register.
    match computed.through {
        ComputedAccess::Descriptor { first, .. } => read(first, 4),
        ComputedAccess::Pointer { base, .. } => read(base, 2),
    }
    if u16::from(computed.soffset) < SCALAR_REGISTERS {
        read(u16::from(computed.soffset), 1);
    }
    Some(mask)
}

/// The scalar registers an instruction reads, each a first register and how many from it; [`None`]
/// for one that reads registers its operands do not name.
///
/// A destination is left out: the first operand of the scalar ALU and load families, and of a
/// vector instruction that writes a scalar (a compare's mask, a lane read), and the second of a
/// vector instruction with a carry or scale out. One that also reads its destination keeps it: a
/// conditional move, a bit set, an add or multiply into a constant's register. Widths are taken
/// generously: a pair for a 64-bit scalar operand and for every scalar a vector instruction reads
/// (a lane mask is a pair), four for a buffer descriptor, eight for an image's.
fn scalar_reads(instruction: &Instruction, family: &str, name: &str) -> Option<Vec<(u16, u16)>> {
    if name.starts_with("s_movrel") || name.starts_with("v_movrel") {
        return None;
    }
    let pair = |wide: bool| if wide { 2 } else { 1 };
    let destinations: &[usize] = match family {
        "SOP1" | "SOP2"
            if ![
                "s_cmov",
                "s_bitset",
                "s_setpc",
                "s_swappc",
                "s_rfe",
                "s_cbranch_join",
            ]
            .iter()
            .any(|reads| name.starts_with(reads)) =>
        {
            &[0]
        }
        "SOPK" if matches!(name, "s_movk_i32" | "s_getreg_b32" | "s_call_b64") => &[0],
        "SMEM"
            if name.starts_with("s_load")
                || name.starts_with("s_buffer_load")
                || name.starts_with("s_scratch_load")
                || name.starts_with("s_mem")
                || name.starts_with("s_get_waveid") =>
        {
            &[0]
        }
        "VOP1" | "VOP2" | "VOPC" | "VOP3" | "VOP3P"
            if ["_co_", "div_scale", "mad_u64", "mad_i64"]
                .iter()
                .any(|carry| name.contains(carry)) =>
        {
            &[0, 1]
        }
        "VOP1" | "VOP2" | "VOPC" | "VOP3" | "VOP3P" => &[0],
        _ => &[],
    };
    let mut reads = Vec::new();
    for (position, operand) in instruction.operands.iter().enumerate() {
        let Operand::Scalar(register) = operand else {
            continue;
        };
        if destinations.contains(&position) || *register >= SCALAR_REGISTERS {
            continue;
        }
        let width = match family {
            // A buffer descriptor; a load's base pair, a flat access's, and whatever a vector
            // instruction reads.
            "SMEM" if name.starts_with("s_buffer") => 4,
            "MUBUF" | "MTBUF" if position == 2 => 4,
            "MIMG" => 8,
            "SMEM" | "FLAT" | "VOP1" | "VOP2" | "VOPC" | "VOP3" | "VOP3P" => 2,
            _ => pair(name.contains("64")),
        };
        reads.push((*register, width));
    }
    // A scalar load's offset register is not an operand: the second word's bits 31:25.
    if family == "SMEM"
        && let Some(second) = instruction.second_word
        && let Ok(code) = u16::try_from((second >> 25) & 0x7f)
        && code < SCALAR_REGISTERS
    {
        reads.push((code, 1));
    }
    Some(reads)
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

    /// `s_load_dwordx4 s[0:3], s[4:5], 0x0`, then `s_waitcnt lgkmcnt(0)`: a descriptor loaded from
    /// a table at the stage's first two words.
    const LOAD_DESCRIPTOR: [u32; 3] = [0xf408_0002, 0xfa00_0000, 0xbf8c_c07f];

    /// A descriptor loaded from a table the first two words name depends on those two alone: the
    /// stage's other words - a draw's own constants - do not change what the replay finds.
    #[test]
    fn only_the_words_the_prefix_reads_are_in_the_mask() {
        // Through s[0:3], which the load wrote, with an inline constant offset.
        assert_eq!(mask(&LOAD_DESCRIPTOR, 12, (0, 128 + 16)), Some(0b11));
    }

    /// The access's own registers count: a descriptor still in user data, and an offset register.
    #[test]
    fn the_access_s_descriptor_and_offset_registers_count() {
        // Through s[8:11] (words 4-7), the offset in s14 (word 10).
        assert_eq!(
            mask(&LOAD_DESCRIPTOR, 12, (8, 14)),
            Some(0b11 | 0b1111 << 4 | 1 << 10)
        );
    }

    /// A write is not a read: `s_mov_b32 s6, 0` leaves word 2 out, and `s_mov_b32 s4, s13` reads
    /// word 9.
    #[test]
    fn a_destination_is_not_read() {
        let mut prefix = vec![0xbe86_0380, 0xbe84_030d];
        prefix.extend(LOAD_DESCRIPTOR);
        assert_eq!(mask(&prefix, 12, (0, 125)), Some(0b11 | 1 << 9));
    }

    /// An add into a register reads it: `s_addk_i32 s6, 1` reads word 2.
    #[test]
    fn an_add_into_a_register_reads_it() {
        assert_eq!(mask(&[0xb786_0001], 12, (0, 125)), Some(0b100));
    }

    /// An instruction the tables do not name may read anything: `s_cmov_b32 s6, s7`.
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
