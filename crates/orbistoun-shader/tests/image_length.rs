//! An image instruction is not always eight bytes, and the decoder has to know.
//!
//! The image family may name its address registers **individually** rather than as a consecutive
//! range. The extra register numbers go in dwords appended to the instruction, and two bits of
//! the first word say how many. A reference compiler emits that form readily - it does so in
//! preference to moving a register, which it did on the first program written to ask - so it is
//! not a corner nobody reaches.
//!
//! Reading such an instruction as eight bytes starts the next decode in the middle of this one
//! and desynchronises the rest of the shader. That is the same failure a missed trailing literal
//! causes, and `hostile.rs` guards the literal case for exactly this reason.
//!
//! # Where the bytes came from
//!
//! Measured, not written. Each encoding below is what the reference assembler produced for the
//! instruction in its comment, on this project's target (worklog 577).

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
/// The first is the ordinary consecutive form and is eight bytes. The rest name their addresses
/// individually, and their first bytes differ from it **only** in the two bits that say how many
/// extra dwords follow - which is what makes this a measurement of that field rather than of
/// four unrelated instructions.
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

/// **Each of these decodes as exactly one instruction, of exactly its own length.**
///
/// Two claims per case, and the second is the one that matters. A decoder reading a twelve-byte
/// instruction as eight would report *two* instructions here, the second assembled out of the
/// tail of the first - and it would keep doing that for the rest of the shader. Asserting the
/// count alone would not catch a length that is wrong in a way that happens to land on a
/// boundary, so the length is asserted too.
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

/// **A stream of them stays in step.**
///
/// The failure this is really about is not one instruction being reported at the wrong length -
/// it is everything after it being garbage. So the four are concatenated and the decode has to
/// find four, in order, at the right offsets.
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
