//! A translated export, checked against the framebuffer (D549).
//!
//! A guest instruction is decoded and translated, and the pixels it produces are compared with the
//! pixels the hand-written oracle shader produces from the same colour, so neither side checks
//! itself.

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

/// A shader that moves four constants into `v0..v3` and exports them to `mrt0`, written as guest
/// instruction words.
fn export_shader(colour: [f32; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    // `v_mov_b32_e32 vN, literal`: VOP1 opcode 1 with the 0xFF literal source, so the value follows
    // the instruction word.
    for (register, component) in colour.iter().enumerate() {
        let word = 0x7E00_0000u32 | ((register as u32) << 17) | (1 << 9) | 0xFF;
        bytes.extend(word.to_le_bytes());
        bytes.extend(component.to_bits().to_le_bytes());
    }
    // `exp mrt0 v0, v1, v2, v3`: target zero, all four channels enabled (`EN`, bits 0-3), four
    // sources one byte each. An export that enables no channel writes nothing.
    bytes.extend(0xF800_000Fu32.to_le_bytes());
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

/// A translated export puts on the screen the same colour a hand-written shader puts there.
///
/// Two draws over the same red clear, one with `constant_colour_fragment_module` and one with a
/// module translated from guest instruction words: every pixel of the two images is equal, and
/// equal to the requested blue. Comparing the images is a differential; asserting the bytes too
/// says which side moved. Only `mrt0` is accepted, because which attachment another target selects
/// is register state (D104). Every lane holds the same value here, so which lane the export reads
/// is not observable.
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

/// An export into a compute dispatch is refused rather than written somewhere.
///
/// The control for the test above: a compute module has no colour attachment, and a store to
/// somewhere arbitrary would render a plausible wrong frame. The refusal's wording is not pinned.
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
