//! An image instruction is not always eight bytes, and the decoder has to know.
//!
//! The image family may name its address registers individually rather than as a
//! consecutive range; the extra register numbers go in dwords appended to the instruction,
//! and two bits of the first word count them. A reference compiler emits this form readily.
//! Reading such an instruction as eight bytes desynchronises the rest of the shader, as a
//! missed trailing literal does (`hostile.rs`). Each encoding below is the reference
//! assembler's output for the printed instruction on this project's target.

use orbistoun_shader::{EncodingTable, OperandTable, decode};

/// One measured instruction: what it assembles to, and what it is.
struct Measured {
    /// The assembler's own bytes.
    encoding: &'static [u8],
    /// What the disassembler printed, for a reader checking the bytes by eye.
    printed: &'static str,
}

/// The four forms that establish the rule, from the same assembler run.
///
/// The first is the ordinary consecutive form, eight bytes. The rest name their addresses
/// individually and differ from it in their first bytes only in the two bits counting extra
/// dwords, so together they measure that field.
const MEASURED: [Measured; 4] = [
    Measured {
        encoding: &[0x08, 0x01, 0x90, 0xf0, 0x00, 0x00, 0x61, 0x00],
        printed: "image_sample_l v0, v[0:2], s[4:11], s[12:15] dmask:0x1 dim:SQ_RSRC_IMG_2D",
    },
    Measured {
        encoding: &[
            0x0a, 0x01, 0x90, 0xf0, 0x02, 0x00, 0x61, 0x00, 0x01, 0x00, 0x00, 0x00,
        ],
        printed: "image_sample_l v0, [v2, v1, v0], s[4:11], s[12:15] dmask:0x1 dim:SQ_RSRC_IMG_2D",
    },
    Measured {
        encoding: &[
            0x0c, 0x01, 0x88, 0xf0, 0x00, 0x00, 0x61, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x00,
            0x00, 0x00,
        ],
        printed: "image_sample_d v0, [v0, v1, v2, v3, v4, v5], s[4:11], s[12:15] dmask:0x1",
    },
    Measured {
        encoding: &[
            0x14, 0x01, 0x88, 0xf0, 0x00, 0x00, 0x61, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06,
            0x07, 0x08,
        ],
        printed: "image_sample_d v0, [v0..v8], s[4:11], s[12:15] dmask:0x1 dim:SQ_RSRC_IMG_3D",
    },
];

/// Each of these decodes as exactly one instruction, of exactly its own length.
///
/// The length is asserted as well as the count, since a wrong length can land on a
/// boundary.
#[test]
fn an_image_instruction_is_as_long_as_the_assembler_made_it() {
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");

    for case in MEASURED {
        let decoded = decode(case.encoding, &encodings, &operands);
        assert_eq!(
            decoded.instructions.len(),
            1,
            concat!(
                "{}: decoded as {} instructions rather than one - a length read short starts ",
                "the next decode inside this one"
            ),
            case.printed,
            decoded.instructions.len()
        );
        let instruction = &decoded.instructions[0];
        assert_eq!(
            instruction.length as usize,
            case.encoding.len(),
            "{}: decoded length {} against {} bytes of encoding",
            case.printed,
            instruction.length,
            case.encoding.len()
        );
        assert!(
            !decoded.desynchronised,
            "{}: the decode desynchronised",
            case.printed
        );
    }
}

/// A stream of them stays in step: four concatenated decode as four, at their offsets.
#[test]
fn a_stream_of_image_instructions_stays_in_step() {
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");

    let mut bytes = Vec::new();
    let mut offsets = Vec::new();
    for case in MEASURED {
        offsets.push(u32::try_from(bytes.len()).expect("a small fixture"));
        bytes.extend_from_slice(case.encoding);
    }

    let decoded = decode(&bytes, &encodings, &operands);
    assert!(!decoded.desynchronised, "the decode desynchronised");
    assert_eq!(
        decoded.instructions.len(),
        MEASURED.len(),
        "four instructions in, {} out",
        decoded.instructions.len()
    );
    let found: Vec<u32> = decoded
        .instructions
        .iter()
        .map(|instruction| instruction.offset)
        .collect();
    assert_eq!(
        found, offsets,
        "the instructions are not where they were put, so a length was read wrong"
    );
}
