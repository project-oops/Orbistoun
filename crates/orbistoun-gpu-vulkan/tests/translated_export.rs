//! A **translated** export, checked against the framebuffer.
//!
//! # The first end-to-end check this project has been able to make
//!
//! Everything in phase 6 is verified against material this project generated, so it cannot be
//! wrong in a way its own tests would notice. The framebuffer oracle was built to break that -
//! it draws a hand-written fragment shader's colour and reads it back, and nothing translated
//! is involved in it (D549, D550).
//!
//! This is the first thing to go through it. A guest instruction is decoded, translated, and
//! the pixels it produces are compared against the pixels the hand-written shader produced from
//! the same colour. Neither side is checking itself.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::draw_with;
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_spirv::{constant_colour_fragment_module, fullscreen_triangle_vertex_module};
use orbistoun_translate::wavefront::{Stage, translate_for};
use orbistoun_translate::{TranslateError, Width};

fn device_or_skip(what: &str) -> bool {
    match probe() {
        Availability::Available { properties } => {
            println!("[{what}] device: {}", properties.device);
            true
        }
        Availability::Unavailable { reason } => {
            println!("[{what}] SKIPPED - no device: {reason}");
            false
        }
    }
}

/// A shader that moves four constants into `v0..v3` and exports them to `mrt0`.
///
/// Written as guest instruction words rather than assembled from anything, so what is being
/// translated is the guest's encoding and not a convenience.
fn export_shader(colour: [f32; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    // `v_mov_b32_e32 vN, literal` - VOP1 opcode 1 with the 0xFF literal source, so the value
    // follows the instruction word.
    for (register, component) in colour.iter().enumerate() {
        let word = 0x7E00_0000u32 | ((register as u32) << 17) | (1 << 9) | 0xFF;
        bytes.extend(word.to_le_bytes());
        bytes.extend(component.to_bits().to_le_bytes());
    }
    // `exp mrt0 v0, v1, v2, v3` - target zero, four sources one byte each.
    bytes.extend(0xF800_0000u32.to_le_bytes());
    bytes.extend(0x0302_0100u32.to_le_bytes());
    // `s_endpgm`.
    bytes.extend(0xBF81_0000u32.to_le_bytes());
    bytes
}

fn translated(colour: [f32; 4], stage: Stage) -> Result<Vec<u32>, TranslateError> {
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");
    let decoded = decode(&export_shader(colour), &encodings, &operands);
    translate_for(
        &decoded,
        &encodings,
        Width::Wave64,
        stage,
        orbistoun_translate::wavefront::Window::default(),
    )
    .map(|(module, _)| module)
}

/// **A translated export puts its colour on the screen, and it is the same colour a
/// hand-written shader puts there.**
///
/// # What this asserts
///
/// Two draws over the same red clear. One uses `constant_colour_fragment_module`, assembled by
/// hand and the shader the oracle was built with; the other uses a module the translator
/// produced from guest instruction words. **Every pixel of the two images is equal**, and equal
/// to the colour asked for rather than to the clear.
///
/// The clear is red and the colour is blue, so a translation that produced nothing, or a
/// pipeline that never bound, reads as red rather than as an absent answer.
///
/// Comparing the two images rather than only the expected bytes is the point: it is a
/// *differential*, and it is the shape every later comparison in this phase takes. Asserting
/// the bytes as well means a failure says which of the two moved.
///
/// # What it cannot assert
///
/// **That anything but a constant export translates.** The shader is four moves and an export.
/// Nothing here exercises interpolation, a real fragment's inputs, blending, depth, more than
/// one attachment, or any target but `mrt0` - and `mrt0` is the only one the translator accepts,
/// because which attachment another target selects is register state a capture would settle and
/// D104 refuses to invent.
///
/// It also says nothing about the *bits* of the export beyond the four sources: the compressed
/// and done flags and the write mask live in the instruction's first word, are not among the
/// operands the decoder solved, and are not read.
///
/// **And it cannot see which lane the export reads.** Changing the translation to take lane one
/// instead of lane zero leaves this passing, because the shader sets its registers with literal
/// moves and a literal move writes every active lane - so every lane holds the same value and
/// the choice is unobservable here. Distinguishing them needs a shader whose lanes differ, which
/// needs an input that varies across a fragment, which is interpolation and is not built. Said
/// aloud because the break was tried and did not fire (D553).
#[test]
fn a_translated_export_matches_the_hand_written_shader() {
    if !device_or_skip("a_translated_export_matches_the_hand_written_shader") {
        return;
    }
    let (width, height) = (8, 5);
    let blue = [0.0, 0.0, 1.0, 1.0];
    let red = [1.0, 0.0, 0.0, 1.0];

    let vertex = fullscreen_triangle_vertex_module();
    let by_hand = draw_with(
        &vertex,
        &constant_colour_fragment_module(blue),
        red,
        width,
        height,
    )
    .expect("the hand-written draw ran");
    let module = translated(blue, Stage::Fragment).expect("the export translated");
    let by_translation =
        draw_with(&vertex, &module, red, width, height).expect("the translated draw ran");

    assert_eq!(
        by_translation.bytes, by_hand.bytes,
        "the translated export and the hand-written shader disagree about the frame"
    );
    for y in 0..height {
        for x in 0..width {
            assert_eq!(
                by_translation.at(x, y),
                Some([0, 0, 255, 255]),
                concat!(
                    "pixel ({}, {}) - red here would mean the export produced nothing and the ",
                    "clear survived"
                ),
                x,
                y
            );
        }
    }
}

/// **An export into a compute dispatch is refused rather than written somewhere.**
///
/// The control for the test above. A compute module has no colour attachment, so there is
/// nowhere for an export to go - and the failure that matters is not an error but a *store to
/// somewhere arbitrary*, which would render a frame that looks plausible and is not.
///
/// It cannot assert what the refusal says, only that there is one; the message is prose and
/// pinning it would make this a test of the wording.
#[test]
fn an_export_without_an_attachment_is_refused() {
    let refusal = translated([0.0, 0.0, 1.0, 1.0], Stage::Compute);
    assert!(
        refusal.is_err(),
        concat!(
            "a compute module translated an export, so it stored a colour somewhere that is ",
            "not an attachment"
        )
    );
}
