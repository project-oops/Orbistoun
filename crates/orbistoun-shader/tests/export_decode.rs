//! The export's operand layout, pinned.
//!
//! # Why this test exists
//!
//! The roadmap listed *"solve the export's operand layout by probe"* as work still to do, and
//! it had been done - `opcode-operands.toml` carries the EXP entry, solved from ten samples,
//! and the decoder reads it. Nothing said so, so the item sat in a table of next steps and in
//! every summary generated from it (D551).
//!
//! A solved layout that nothing asserts can also be *lost* - a regenerated table that dropped
//! the entry would take `exp` back to decoding nothing, and the only symptom would be a
//! worklist quietly reranking. This is the assertion that was missing.

use orbistoun_shader::{EncodingTable, Operand, OperandTable, decode};

/// An EXP instruction with the given target and four consecutive source registers.
///
/// The encoding is the one `encodings.toml` declares: mask `0xFC000000`, value `0xF8000000`,
/// eight bytes. The target is six bits at shift four; the four sources are one byte each in
/// the second word.
fn export_words(target: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend((0xF800_0000u32 | (target << 4)).to_le_bytes());
    bytes.extend(0x0302_0100u32.to_le_bytes());
    bytes
}

/// **`exp` decodes to a target and four vector registers.**
///
/// # What this asserts
///
/// That the export's operands come back decoded rather than empty, and that the target is read
/// as a value rather than fixed - three targets, three answers. `operands_decoded` is checked
/// separately from the operand list because an empty list with the flag set means "this family
/// genuinely takes none", which is a different claim from "nobody has taught the decoder this
/// family" and the decoder is careful to distinguish them.
///
/// # What it cannot assert
///
/// **What the target numbers mean.** `Immediate(0)` is the first render target by the
/// mnemonic's own name, but which attachment that becomes, and what targets 8 and above select,
/// is not decided here and is not decided by decoding. That is the mapping the roadmap's step
/// (d) says needs a capture, and D104 refuses to invent.
///
/// It also says nothing about *translation*. `exp` is still in `BLOCKED`, for a reason that is
/// now precisely one thing: every module the translator emits is a compute dispatch, so an
/// export has nowhere to go inside it.
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
