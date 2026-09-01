//! The decoder against bytes that are not a shader.
//!
//! # Why this matters before any real data arrives
//!
//! Every other test in this crate feeds the decoder something that *is* an instruction
//! stream - a compiled fixture, or a word this project built on purpose. Real material
//! will not be so obliging. A shader address decoded from the wrong register bits points
//! at a texture, a stack frame, or nothing; a branch target computed wrongly lands
//! mid-instruction; a shader read from guest memory runs past its end into whatever the
//! guest put there.
//!
//! In all of those the decoder is handed bytes that are not instructions, and its job is
//! to **say so** rather than to hang, panic, or quietly report a plausible program. The
//! last is the dangerous one: a decode that runs off into garbage and produces a hundred
//! confident instructions would send the worklist chasing opcodes nobody's guest ever
//! executed.
//!
//! # The properties
//!
//! For *any* input, however hostile:
//!
//! - it terminates
//! - it does not panic
//! - offsets are strictly increasing and inside the buffer
//! - garbage is reported as untrustworthy rather than presented as a program
//!
//! Termination is the one that would be catastrophic to get wrong and the easiest to get
//! wrong: an instruction whose decoded length is zero advances nothing, and the loop
//! never ends. There is a guard for exactly that, and this is what proves it.

use orbistoun_shader::{EncodingTable, OperandTable, decode, decode_program};

fn tables() -> (EncodingTable, OperandTable) {
    (
        EncodingTable::builtin().expect("encodings"),
        OperandTable::builtin().expect("operands"),
    )
}

/// A seeded generator, so a failure is reproducible rather than a story.
struct Rng(u64);

impl Rng {
    const fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// Checks the properties that must hold for any input at all.
fn well_formed(bytes: &[u8], decoded: &orbistoun_shader::Decode, what: &str) {
    let mut previous: Option<u32> = None;
    for instruction in &decoded.instructions {
        assert!(
            (instruction.offset as usize) < bytes.len().max(1),
            "{what}: an instruction at {:#x} starts past the end of a {}-byte buffer",
            instruction.offset,
            bytes.len()
        );
        if let Some(previous) = previous {
            assert!(
                instruction.offset > previous,
                "{what}: offsets did not advance - {previous:#x} then {:#x}. A decode \
                 that does not move forward does not terminate",
                instruction.offset
            );
        }
        assert!(
            instruction.length > 0,
            "{what}: an instruction at {:#x} claims zero length, which is the shape of \
             an endless loop",
            instruction.offset
        );
        previous = Some(instruction.offset);
    }
}

#[test]
fn random_bytes_decode_without_panicking_or_hanging() {
    // The blunt property. If this ever hangs rather than fails, the guard against a
    // zero-length instruction has gone - and a hung test is the symptom a hung emulator
    // would have.
    let (table, operands) = tables();
    let mut rng = Rng(0x5EED);

    for round in 0..512 {
        let length = (rng.next() % 256) as usize;
        let bytes: Vec<u8> = (0..length).map(|_| (rng.next() >> 24) as u8).collect();

        let decoded = decode(&bytes, &table, &operands);
        well_formed(&bytes, &decoded, &format!("round {round}"));

        // And the same buffer read as a shader with no declared length.
        let program = decode_program(&bytes, &table, &operands);
        well_formed(&bytes, &program, &format!("round {round} as a program"));
        assert!(
            program.consumed <= bytes.len(),
            "round {round}: consumed {} of {} bytes",
            program.consumed,
            bytes.len()
        );
    }
}

#[test]
fn a_buffer_of_zeros_is_not_reported_as_a_hundred_instructions() {
    // The shape a wrong shader address most often takes: mapped memory that is not code.
    // Zeros happen to decode as *something* in most encodings, so the honest answer is
    // not "no instructions" - it is a decode flagged as untrustworthy.
    let (table, operands) = tables();
    let bytes = vec![0u8; 512];
    let decoded = decode(&bytes, &table, &operands);
    well_formed(&bytes, &decoded, "zeros");

    assert!(
        !decoded.is_trustworthy()
            || decoded
                .instructions
                .iter()
                .all(orbistoun_shader::Instruction::is_known),
        "a decode of zeros must either be flagged untrustworthy or consist entirely of \
         instructions the table recognises - anything else is a confident guess"
    );
}

#[test]
fn a_buffer_of_ones_is_handled() {
    // The other degenerate case, and the one most likely to produce absurd lengths.
    let (table, operands) = tables();
    let bytes = vec![0xFFu8; 512];
    let decoded = decode(&bytes, &table, &operands);
    well_formed(&bytes, &decoded, "ones");
}

#[test]
fn the_typed_buffer_half_precision_variants_decode_distinctly() {
    // This test used to assert the opposite, and that was the point of it.
    //
    // The typed-buffer opcode is split - bits 18:16 of the first word and a fourth at
    // bit 53, which is bit 21 of the second - and the table read only the contiguous
    // part, so every half-precision variant decoded as the operation it is a variant of.
    // The gap was pinned as a *passing* test asserting the conflation existed, so that
    // closing it would fail here and say so rather than leaving a comment describing a
    // problem somebody had already fixed.
    //
    // It did exactly that. Kept, inverted, as the guard that the fourth bit is still read.
    //
    // These two encodings came from the reference assembler for this target:
    //
    //   tbuffer_load_format_x      -> e8a02000 80020001
    //   tbuffer_load_format_d16_x  -> e8a02000 80220001
    //
    // Identical first word. The whole difference is bit 21 of the second.
    let (table, operands) = tables();

    let plain = [0xe8a0_2000u32, 0x8002_0001];
    let half = [0xe8a0_2000u32, 0x8022_0001];
    let decode_one = |words: [u32; 2]| {
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decoded = decode(&bytes, &table, &operands);
        let instruction = decoded
            .instructions
            .first()
            .expect("one instruction")
            .clone();
        (instruction.encoding, instruction.opcode)
    };

    let (plain_family, plain_opcode) = decode_one(plain);
    let (half_family, half_opcode) = decode_one(half);

    assert_eq!(
        plain_family, half_family,
        "both are typed-buffer instructions and must land in the same family"
    );
    assert_ne!(
        plain_opcode, half_opcode,
        "the fourth opcode bit is not being read - these differ only in bit 53 and          decoding them alike means every half-precision variant is reported as the          operation it is a variant of"
    );
    // The continuation is the *high* bit, so the variant is its counterpart plus eight.
    // Asserting the arithmetic rather than just inequality catches a continuation
    // shifted to the wrong place, which would still produce two different numbers.
    assert_eq!(
        half_opcode,
        plain_opcode + 8,
        "the fourth bit should be the opcode's high bit"
    );
}

#[test]
fn a_truncated_instruction_is_reported_as_overrunning() {
    // A shader read from guest memory can end at a page boundary part way through an
    // eight-byte instruction. Continuing would read whatever follows in memory as
    // operands.
    let (table, operands) = tables();
    // Half of an eight-byte instruction, whichever family the table says is one.
    //
    // Asked rather than written down. This test used to hold a scalar-load word from a
    // different architecture generation; after a retarget that word matched no family at
    // all, so it decoded as four unrecognised bytes, did not overrun, and the test failed
    // while reporting the decoder as broken.
    let wide = table
        .encodings()
        .iter()
        .find(|encoding| encoding.width_bytes == 8)
        .expect("some family is eight bytes wide");
    let bytes = wide.value.to_le_bytes().to_vec();

    let decoded = decode(&bytes, &table, &operands);
    well_formed(&bytes, &decoded, "truncated");
    assert!(
        decoded.overran,
        "an instruction claiming more bytes than exist must set `overran`, or a caller \
         cannot tell a complete shader from a clipped one"
    );
    assert!(!decoded.is_trustworthy());
}

#[test]
fn a_shader_with_no_terminator_is_not_silently_complete() {
    // `decode_program` reads a window rather than a shader, so it has to distinguish
    // "the program ended here" from "the window ran out". Confusing the two means a
    // shader read from a wrong address looks like a short, valid one.
    let (table, operands) = tables();
    // Two moves and nothing that ends the program.
    let words: [u32; 2] = [0x7E00_0280, 0x7E02_0280];
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();

    let program = decode_program(&bytes, &table, &operands);
    assert!(
        !program.terminated,
        "no end-of-program instruction was present, so `terminated` must be false"
    );

    // And with one, it is.
    let words: [u32; 3] = [0x7E00_0280, 0x7E02_0280, 0xBF81_0000];
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let program = decode_program(&bytes, &table, &operands);
    assert!(program.terminated);
    assert_eq!(program.consumed, bytes.len());
}

#[test]
fn trailing_bytes_that_are_not_a_whole_word_are_reported() {
    // A buffer whose length is not a multiple of four is not a shader, and saying so is
    // cheaper than guessing what the remainder was meant to be.
    let (table, operands) = tables();
    let bytes = vec![0u8; 6];
    let decoded = decode(&bytes, &table, &operands);
    well_formed(&bytes, &decoded, "ragged");
    assert_eq!(decoded.trailing_bytes, 2);
    assert!(!decoded.is_trustworthy());
}

#[test]
fn an_empty_buffer_decodes_to_nothing_rather_than_failing() {
    // Reachable from a zero-length mapping, and the honest answer is an empty program
    // rather than an error - there is nothing wrong with the bytes, there are none.
    let (table, operands) = tables();
    let decoded = decode(&[], &table, &operands);
    assert!(decoded.instructions.is_empty());
    assert!(!decoded.overran);
    assert_eq!(decoded.trailing_bytes, 0);

    let program = decode_program(&[], &table, &operands);
    assert!(!program.terminated, "nothing cannot have ended a program");
    assert_eq!(program.consumed, 0);
}

#[test]
fn every_single_word_decodes_without_panicking() {
    // Exhaustive over the high byte, which is what every encoding family is selected by,
    // and sampled below that. A family whose mask or length rule is malformed shows up
    // here as a panic rather than as a strange fixture months later.
    let (table, operands) = tables();

    for high in 0u32..256 {
        for low in [0x0000_0000u32, 0x00FF_FFFF, 0x0055_5555, 0x00AA_AAAA] {
            let word = (high << 24) | low;
            // Eight bytes, so an instruction claiming a second word finds one.
            let bytes: Vec<u8> = [word, 0].iter().flat_map(|w| w.to_le_bytes()).collect();
            let decoded = decode(&bytes, &table, &operands);
            well_formed(&bytes, &decoded, &format!("word {word:#010x}"));
        }
    }
}

#[test]
fn the_table_cannot_describe_an_instruction_that_advances_nothing() {
    // Termination is guaranteed by construction rather than by the tests above: every
    // recognised instruction advances by its declared width, every unrecognised one by
    // the four-byte minimum, and the loader refuses a width of zero. This pins that
    // refusal, so the guarantee stays a guarantee rather than becoming an accident of
    // what the table happens to contain.
    let malformed = r#"
        [[encoding]]
        name = "BROKEN"
        mask = "0xFC000000"
        value = "0x00000000"
        opcode = { shift = 0, width = 4 }
        width_bytes = 0
    "#;
    let error = EncodingTable::load(malformed)
        .expect_err("a zero-width encoding is an endless loop waiting to happen");
    let text = error.to_string();
    assert!(
        text.contains("BROKEN") || text.contains("width"),
        "the error should name the offending encoding, got: {text}"
    );
}
