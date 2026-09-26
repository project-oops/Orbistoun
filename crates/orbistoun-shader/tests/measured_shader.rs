//! The RDNA2 decoder, held against a shader captured from live GPU memory.
//!
//! `differential.rs` checks the decoder against shaders generated here; this checks it
//! against bytecode obSCEne read from a running system compositor on hardware (check
//! `170-gpu-capture`). obSCEne's own scan reported `endpgm-word 0xbf810000` at
//! `endpgm-offset 0x24`, and captured the 16-byte window `00 01 28 f4 40 00 00 fa 7f c0 8c bf
//! 00 00 2f d5`. The first twelve bytes are an SMEM load and an SOPP `s_waitcnt`; the
//! trailing VOP3 is cut off at the window edge and not walked.

use orbistoun_shader::{EncodingTable, OperandTable, decode};

fn table() -> EncodingTable {
    EncodingTable::builtin().expect("built-in encoding table")
}

fn operands() -> OperandTable {
    OperandTable::builtin().expect("built-in operand table")
}

/// The family name orbistoun assigned an instruction, or a marker if it did not recognise one.
fn family<'a>(inst: &orbistoun_shader::Instruction, table: &'a EncodingTable) -> &'a str {
    inst.encoding
        .and_then(|i| table.encodings().get(usize::from(i)))
        .map_or("<unrecognised>", |e| e.name.as_str())
}

/// The terminator obSCEne found is the one orbistoun decodes.
///
/// obSCEne identified `0xbf810000` as `s_endpgm` by its own scan; the decoder must read the
/// same word as SOPP opcode 1, `s_endpgm`, or the two disagree on where a shader ends.
#[test]
fn the_terminator_obscene_identified_is_the_one_orbistoun_decodes() {
    let table = table();
    let bytes = 0xbf81_0000_u32.to_le_bytes();
    let decoded = decode(&bytes, &table, &operands());

    let inst = decoded.instructions.first().expect("one instruction");
    assert_eq!(family(inst, &table), "SOPP", "s_endpgm is an SOPP");
    assert_eq!(inst.opcode, 1, "SOPP opcode 1");
    assert_eq!(
        table.mnemonic_for("SOPP", inst.opcode),
        Some("s_endpgm"),
        "the word obSCEne read as the program terminator",
    );
}

/// The captured window's complete instructions decode as RDNA2.
///
/// The first twelve bytes are an SMEM at offset 0 (eight bytes, `f4280100 fa000040`) and an
/// SOPP `s_waitcnt` at offset 8 (four bytes, `bf8cc07f`, opcode 0x0c).
#[test]
fn the_captured_window_decodes_as_real_rdna2() {
    let table = table();
    // The twelve complete bytes; the thirteenth-onward VOP3 is truncated by the window edge.
    let bytes = [
        0x00, 0x01, 0x28, 0xf4, 0x40, 0x00, 0x00, 0xfa, 0x7f, 0xc0, 0x8c, 0xbf,
    ];
    let decoded = decode(&bytes, &table, &operands());

    assert!(
        !decoded.desynchronised,
        "both complete instructions were recognised, so nothing had to be guessed past",
    );
    assert_eq!(decoded.instructions.len(), 2, "an SMEM then an SOPP");

    let smem = &decoded.instructions[0];
    assert_eq!(family(smem, &table), "SMEM", "the load at offset 0");
    assert_eq!(smem.offset, 0);
    assert_eq!(smem.length, 8, "SMEM is eight bytes, header plus one");

    let sopp = &decoded.instructions[1];
    assert_eq!(sopp.offset, 8, "the SOPP begins where the SMEM ends");
    assert_eq!(family(sopp, &table), "SOPP");
    assert_eq!(
        table.mnemonic_for("SOPP", sopp.opcode),
        Some("s_waitcnt"),
        "SOPP opcode 0x0c, as the captured bytes encode",
    );
}
