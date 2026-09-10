//! orbistoun's RDNA2 decoder, held against a shader captured from live GPU memory.
//!
//! # Why this is a different kind of evidence from the fixtures
//!
//! `differential.rs` checks the decoder against shaders this project generated and disassembled
//! with a reference tool - strong for coverage, but every byte in it was produced here. This is
//! bytecode obSCEne read out of a **running compositor** (`AgcCompositor.elf`) on hardware
//! (`170-gpu-capture`, `payload-klog.obs.log`): a real title's real shader, not a fixture.
//!
//! # The one instruction obSCEne decoded, and the window it did not
//!
//! obSCEne scanned the captured shader for the program terminator and reported
//! `endpgm-word 0xbf810000` at `endpgm-offset 0x24` - its own independent identification of
//! `s_endpgm`. That is the cross-check: two readers agreeing on one live-hardware instruction,
//! the shader-side analog of the PM4-header agreement in `orbistoun-gpu`'s `measured_packets`.
//!
//! It also captured a 16-byte window of the bytecode, `00 01 28 f4 40 00 00 fa 7f c0 8c bf 00 00
//! 2f d5`, which it did not decode instruction by instruction. orbistoun does, and the first two
//! are complete: an SMEM load (eight bytes) and an SOPP `s_waitcnt` (four). The trailing VOP3 is
//! cut off at the window edge, so only those twelve bytes are walked - the point is the decode of
//! real captured bytes, not the truncated tail.

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

/// **The terminator obSCEne found is the one orbistoun decodes.**
///
/// obSCEne identified `0xbf810000` as `s_endpgm` by its own scan of the shader (it is what
/// `170-gpu-capture/shader-blob` searches for and reports as `endpgm-word`). orbistoun's decoder,
/// given the same word, must call it SOPP opcode 1, `s_endpgm` - or the two disagree on where a
/// real shader ends, which is where the coverage ranking anchors.
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

/// **The captured window's complete instructions decode as real RDNA2.**
///
/// The first twelve bytes of obSCEne's captured window are two whole instructions: an SMEM at
/// offset 0 (eight bytes, `f4280100 fa000040`) and an SOPP `s_waitcnt` at offset 8 (four bytes,
/// `bf8cc07f`, opcode 0x0c). Decoding real bytecode from live memory into the right families is
/// the thing the fixtures cannot prove, because they were made here.
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
