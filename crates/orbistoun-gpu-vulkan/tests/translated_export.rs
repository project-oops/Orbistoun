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

/// A shader that sets the execution mask to `exec` (`s_mov_b64 exec, <inline>`), moves `colour`
/// into `v0..v3` and makes the pixel shader's final export - `done` and `vm` set, as the last
/// colour export of every pixel shader is (Mesa `aco_assembler.cpp:1000`, `:1474-1475`).
fn final_export_shader(exec_inline: u32, colour: [f32; 4]) -> Vec<u8> {
    // `s_mov_b64 exec, <inline>`: SOP1 opcode 4, destination 126 (exec).
    let mut bytes = (0xBE80_0000u32 | (126 << 16) | (4 << 8) | exec_inline)
        .to_le_bytes()
        .to_vec();
    for (register, component) in colour.iter().enumerate() {
        let word = 0x7E00_0000u32 | ((register as u32) << 17) | (1 << 9) | 0xFF;
        bytes.extend(word.to_le_bytes());
        bytes.extend(component.to_bits().to_le_bytes());
    }
    // `exp mrt0 v0, v1, v2, v3 done vm`: `DONE` bit 11, `VM` bit 12.
    bytes.extend((0xF800_000Fu32 | (1 << 11) | (1 << 12)).to_le_bytes());
    bytes.extend(0x0302_0100u32.to_le_bytes());
    bytes.extend(0xBF81_0000u32.to_le_bytes());
    bytes
}

/// A pixel whose lane is inactive at the valid-mask export is discarded: with the execution mask
/// cleared the red clear survives; with it set, the export's blue is written. This is how a
/// compiler discards - Mesa's ACO clears the lane's execution bit before the final export
/// (`aco_select_nir_intrinsics.cpp:4676`) - and how Bugdom 2's alpha test drops a keyed texel
/// (`v_cmp_neq_f32 vcc, alpha, 0; s_and_b32 exec_lo, exec_lo, vcc_lo; exp ... done vm`).
#[test]
fn an_inactive_pixel_at_the_final_export_is_discarded() {
    if !device_or_skip("an_inactive_pixel_at_the_final_export_is_discarded") {
        return;
    }
    let (width, height) = (8, 5);
    let (blue, red) = ([0.0, 0.0, 1.0, 1.0], [1.0, 0.0, 0.0, 1.0]);
    let vertex = fullscreen_triangle_vertex_module();
    let draw = |exec_inline: u32| {
        let encodings = EncodingTable::builtin().expect("the shipped encoding table");
        let operands = OperandTable::builtin().expect("the shipped operand table");
        let decoded = decode(
            &final_export_shader(exec_inline, blue),
            &encodings,
            &operands,
        );
        let (module, _) = translate_for(
            &decoded,
            &encodings,
            Width::Wave64,
            Stage::Fragment,
            orbistoun_translate::wavefront::Window::default(),
        )
        .expect("the final export translated");
        draw_with(&vertex, &module, red, width, height).expect("the draw ran")
    };
    // Inline constants: 128 is 0, 193 is -1.
    let (cleared, set) = (draw(128), draw(193));
    for y in 0..height {
        for x in 0..width {
            assert_eq!(
                cleared.at(x, y),
                Some([255, 0, 0, 255]),
                "({x}, {y}) discarded"
            );
            assert_eq!(set.at(x, y), Some([0, 0, 255, 255]), "({x}, {y}) written");
        }
    }
}

/// ACO's early exit from a pixel shader - every lane's execution bit cleared by a `discard`, then
/// `exp null off, off, off, off done vm` (`aco_lower_to_hw_instr.cpp:2471`) - writes nothing and
/// discards the pixel: the red clear survives.
#[test]
fn a_null_export_discards_an_inactive_pixel() {
    if !device_or_skip("a_null_export_discards_an_inactive_pixel") {
        return;
    }
    let (width, height) = (8, 5);
    // `s_mov_b64 exec, 0`, then `exp null` (target 9 at bit 4) with no channels, `done` and `vm`.
    let mut bytes = (0xBE80_0000u32 | (126 << 16) | (4 << 8) | 128)
        .to_le_bytes()
        .to_vec();
    bytes.extend((0xF800_0000u32 | (9 << 4) | (1 << 11) | (1 << 12)).to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes.extend(0xBF81_0000u32.to_le_bytes());
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");
    let decoded = decode(&bytes, &encodings, &operands);
    let (module, _) = translate_for(
        &decoded,
        &encodings,
        Width::Wave64,
        Stage::Fragment,
        orbistoun_translate::wavefront::Window::default(),
    )
    .expect("the null export translated");
    let red = [1.0, 0.0, 0.0, 1.0];
    let drawn = draw_with(
        &fullscreen_triangle_vertex_module(),
        &module,
        red,
        width,
        height,
    )
    .expect("the draw ran");
    for y in 0..height {
        for x in 0..width {
            assert_eq!(
                drawn.at(x, y),
                Some([255, 0, 0, 255]),
                "({x}, {y}) discarded"
            );
        }
    }
}

/// An export that enables some channels writes those and keeps the others as the output held them:
/// blue exported whole, then red alone, leaves red over the blue's green, blue and alpha. Mesa
/// exports a one-byte red target's colour this way (`SPI_SHADER_32_R`, `ac_shader_util.c`), and the
/// target takes only red.
#[test]
fn a_partial_export_writes_only_its_channels() {
    if !device_or_skip("a_partial_export_writes_only_its_channels") {
        return;
    }
    let (width, height) = (8, 5);
    let mut bytes = Vec::new();
    for (register, component) in [0.0f32, 0.0, 1.0, 1.0].iter().enumerate() {
        let word = 0x7E00_0000u32 | ((register as u32) << 17) | (1 << 9) | 0xFF;
        bytes.extend(word.to_le_bytes());
        bytes.extend(component.to_bits().to_le_bytes());
    }
    bytes.extend(0xF800_000Fu32.to_le_bytes());
    bytes.extend(0x0302_0100u32.to_le_bytes());
    // `v_mov_b32 v0, 1.0`, then `exp mrt0 v0, off, off, off`: `EN` 0x1, red alone.
    bytes.extend(0x7E00_02FFu32.to_le_bytes());
    bytes.extend(1.0f32.to_bits().to_le_bytes());
    bytes.extend(0xF800_0001u32.to_le_bytes());
    bytes.extend(0x0000_0000u32.to_le_bytes());
    bytes.extend(0xBF81_0000u32.to_le_bytes());

    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");
    let decoded = decode(&bytes, &encodings, &operands);
    let (module, _) = translate_for(
        &decoded,
        &encodings,
        Width::Wave64,
        Stage::Fragment,
        orbistoun_translate::wavefront::Window::default(),
    )
    .expect("a partial export translates");
    let vertex = fullscreen_triangle_vertex_module();
    let drawn = draw_with(&vertex, &module, [0.0, 1.0, 0.0, 1.0], width, height).expect("drawn");
    for (x, y) in [(0, 0), (7, 4), (3, 2)] {
        assert_eq!(drawn.at(x, y), Some([255, 0, 255, 255]), "({x}, {y})");
    }
}

/// The channels a pixel shader exports are its `mrt0` exports' `EN` fields together: red alone for
/// an `exp mrt0 v0, off, off, off`, all four for a whole export.
#[test]
fn a_shader_s_exported_channels_are_its_exports_enables() {
    use orbistoun_translate::wavefront::exported_colour_channels;
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");
    let mut red = 0x7E00_02FFu32.to_le_bytes().to_vec();
    red.extend(1.0f32.to_bits().to_le_bytes());
    red.extend(0xF800_0001u32.to_le_bytes());
    red.extend(0x0000_0000u32.to_le_bytes());
    red.extend(0xBF81_0000u32.to_le_bytes());
    assert_eq!(
        exported_colour_channels(&decode(&red, &encodings, &operands), &encodings),
        0x1
    );
    let whole = export_shader([0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        exported_colour_channels(&decode(&whole, &encodings, &operands), &encodings),
        0xF
    );
}
