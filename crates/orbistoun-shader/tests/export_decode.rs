//! The export's operand layout, pinned.
//!
//! `opcode-operands.toml` carries the EXP entry, solved from ten samples. A regenerated
//! table that dropped it would return `exp` to decoding nothing, with a reranked worklist
//! as the only symptom.

use orbistoun_shader::{EncodingTable, Operand, OperandTable, decode};

/// An EXP instruction with the given target and four consecutive source registers.
///
/// The encoding is the one `encodings.toml` declares: mask `0xFC000000`, value
/// `0xF8000000`, eight bytes. The target is six bits at shift four; the four sources are one
/// byte each in the second word.
fn export_words(target: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend((0xF800_0000u32 | (target << 4)).to_le_bytes());
    bytes.extend(0x0302_0100u32.to_le_bytes());
    bytes
}

/// `exp` decodes to a target and four vector registers.
///
/// The target is read as a value, not fixed: three targets, three answers.
/// `operands_decoded` is checked separately, since an empty list with the flag set means the
/// family takes no operands. What the target numbers select (which attachment, and targets 8
/// and above) is not decided by decoding (D104). `exp` is in `BLOCKED` because a translated
/// module is a compute dispatch, which has nowhere to export to.
#[test]
fn an_export_decodes_to_a_target_and_four_registers() {
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");

    for target in [0u32, 1, 12] {
        let decoded = decode(&export_words(target), &encodings, &operands);
        let instruction = decoded
            .instructions
            .first()
            .expect("one instruction from eight bytes");
        assert!(
            instruction.is_known(),
            "the export encoding is not recognised at all, so nothing below is meaningful"
        );
        assert!(
            instruction.operands_decoded,
            concat!(
                "the export's operand layout is not established - it was, solved from ten ",
                "samples, so a regenerated table has lost the EXP entry"
            )
        );
        assert_eq!(
            instruction.operands,
            vec![
                Operand::Immediate(i64::from(target)),
                Operand::Vector(0),
                Operand::Vector(1),
                Operand::Vector(2),
                Operand::Vector(3),
            ],
            "export to target {target}"
        );
    }
}
