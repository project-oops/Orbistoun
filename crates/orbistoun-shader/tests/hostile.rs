//! The decoder against bytes that are not a shader.
//!
//! A wrong shader address, a mis-computed branch target, or a shader read past its end all
//! hand the decoder bytes that are not instructions. For any input it must terminate, not
//! panic, keep offsets strictly increasing and inside the buffer, and flag garbage as
//! untrustworthy rather than present it as a program. Termination rests on no instruction
//! having zero length.

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
                concat!(
                    "{}: offsets did not advance - {:#x} then {:#x}. A decode ",
                    "that does not move forward does not terminate"
                ),
                what,
                previous,
                instruction.offset
            );
        }
        assert!(
            instruction.length > 0,
            concat!(
                "{}: an instruction at {:#x} claims zero length, which is the shape of ",
                "an endless loop"
            ),
            what,
            instruction.offset
        );
        previous = Some(instruction.offset);
    }
}

/// Random bytes decode without panicking or hanging, in both decode modes.
#[test]
fn random_bytes_decode_without_panicking_or_hanging() {
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

/// A buffer of zeros is either flagged untrustworthy or wholly recognised.
#[test]
fn a_buffer_of_zeros_is_not_reported_as_a_hundred_instructions() {
    // Zeros decode as something in most encodings; mapped memory that is not code is the
    // common shape of a wrong shader address.
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
        concat!(
            "a decode of zeros must either be flagged untrustworthy or consist entirely of ",
            "instructions the table recognises - anything else is a confident guess"
        )
    );
}

/// A buffer of `0xFF` bytes decodes well-formed.
#[test]
fn a_buffer_of_ones_is_handled() {
    let (table, operands) = tables();
    let bytes = vec![0xFFu8; 512];
    let decoded = decode(&bytes, &table, &operands);
    well_formed(&bytes, &decoded, "ones");
}

/// The typed-buffer half-precision variants decode as distinct opcodes.
#[test]
fn the_typed_buffer_half_precision_variants_decode_distinctly() {
    // The typed-buffer opcode is split: bits 18:16 of the first word and a fourth at bit
    // 53, bit 21 of the second. From the reference assembler for this target:
    //
    //   tbuffer_load_format_x      -> e8a02000 80020001
    //   tbuffer_load_format_d16_x  -> e8a02000 80220001
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
        concat!(
            "the fourth opcode bit is not being read - these differ only in bit 53 and ",
            "decoding them alike means every half-precision variant is reported as the ",
            "operation it is a variant of"
        )
    );
    // The continuation is the high bit, so the variant is its counterpart plus eight; the
    // arithmetic catches a continuation shifted to the wrong place.
    assert_eq!(
        half_opcode,
        plain_opcode + 8,
        "the fourth bit should be the opcode's high bit"
    );
}

/// Half of an eight-byte instruction sets `overran`.
#[test]
fn a_truncated_instruction_is_reported_as_overrunning() {
    // A shader read from guest memory can end part way through an eight-byte instruction.
    let (table, operands) = tables();
    // Half of an eight-byte instruction, from whichever family the table says is one, so
    // the test survives a retarget.
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
        concat!(
            "an instruction claiming more bytes than exist must set `overran`, or a caller ",
            "cannot tell a complete shader from a clipped one"
        )
    );
    assert!(!decoded.is_trustworthy());
}

/// `decode_program` distinguishes a program that ended from a window that ran out.
#[test]
fn a_shader_with_no_terminator_is_not_silently_complete() {
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

/// A buffer that is not whole words is reported untrustworthy.
#[test]
fn trailing_bytes_that_are_not_a_whole_word_are_reported() {
    let (table, operands) = tables();
    let bytes = vec![0u8; 6];
    let decoded = decode(&bytes, &table, &operands);
    well_formed(&bytes, &decoded, "ragged");
    assert_eq!(decoded.trailing_bytes, 2);
    assert!(!decoded.is_trustworthy());
}

/// An empty buffer decodes to an empty program, not an error.
#[test]
fn an_empty_buffer_decodes_to_nothing_rather_than_failing() {
    let (table, operands) = tables();
    let decoded = decode(&[], &table, &operands);
    assert!(decoded.instructions.is_empty());
    assert!(!decoded.overran);
    assert_eq!(decoded.trailing_bytes, 0);

    let program = decode_program(&[], &table, &operands);
    assert!(!program.terminated, "nothing cannot have ended a program");
    assert_eq!(program.consumed, 0);
}

/// Every high byte, with sampled low bits, decodes well-formed.
#[test]
fn every_single_word_decodes_without_panicking() {
    // The high byte selects every encoding family, so a malformed mask or length rule
    // shows up here as a panic.
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

/// The table loader refuses a zero-width encoding, which would never advance.
#[test]
fn the_table_cannot_describe_an_instruction_that_advances_nothing() {
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
