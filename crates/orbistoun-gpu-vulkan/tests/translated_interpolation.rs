//! A **translated** interpolation, checked against the framebuffer.
//!
//! `translated_export.rs` put a translated constant on the screen. This puts a translated
//! *interpolated attribute* there: the guest reads `attr0.x` through `v_interp_p1_f32`, and the
//! value that arrives is the one the pipeline interpolated from the vertex shader's varying
//! (D555).
//!
//! The oracle gained the varying first, deliberately - a translated interpolation checked
//! against a pipeline that interpolates nothing would have been verified against material this
//! project generated (D554).

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::draw_with;
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_spirv::{interpolated_vertex_module, passthrough_fragment_module};
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

/// A shader that reads four channels of attribute zero and exports them.
///
/// `v_interp_p1_f32 vN, v0, attr0.<chan>` four times, then `exp mrt0 v0, v1, v2, v3`. Written as
/// guest instruction words, so what is translated is the encoding rather than a convenience.
///
/// The VINTRP encoding is `0xC8000000`: destination at shift 18, source at shift 0, attribute at
/// shift 10 and channel at shift 8 - the layout `opcode-operands.toml` solved.
fn interpolating_shader() -> Vec<u8> {
    let mut bytes = Vec::new();
    for (register, channel) in (0u32..4).enumerate() {
        let word = 0xC800_0000u32 | ((register as u32) << 18) | (channel << 8);
        bytes.extend(word.to_le_bytes());
    }
    bytes.extend(0xF800_000Fu32.to_le_bytes());
    bytes.extend(0x0302_0100u32.to_le_bytes());
    bytes.extend(0xBF81_0000u32.to_le_bytes());
    bytes
}

fn translated(stage: Stage) -> Result<Vec<u32>, TranslateError> {
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");
    let decoded = decode(&interpolating_shader(), &encodings, &operands);
    translate_for(
        &decoded,
        &encodings,
        Width::Wave64,
        stage,
        orbistoun_translate::wavefront::Window::default(),
    )
    .map(|(module, _)| module)
}

/// **A translated interpolation reads the value the pipeline interpolated.**
///
/// # What this asserts
///
/// The vertex shader gives all three corners the same colour, so every barycentric weighting of
/// them is that colour and the expected result is exact - it depends on no sample position and
/// no rounding the driver chose. The translated fragment shader interpolates `attr0.x` through
/// `.w`, exports them, and every pixel comes back that colour.
///
/// It is compared against the hand-written passthrough shader over the same geometry as well as
/// against the expected bytes, so a failure says which of the two moved.
///
/// The clear is a colour neither shader writes, so a translation that produced nothing reads as
/// a different answer rather than an absent one.
///
/// # What it cannot assert
///
/// **That `p1` and `p2` mean what this translation says.** Both halves of the guest's pair are
/// translated as the whole interpolated value, on the reasoning that SPIR-V's input variable
/// *is* the result and the two-step computation has no host equivalent. A shader that used
/// `p1`'s intermediate for anything other than feeding `p2` would get a different number here -
/// that is recorded as an assumption in `interpolate`, and this test cannot see it, because the
/// shader it uses does exactly what a compiler emits.
///
/// **Nor that the barycentrics are honoured.** Both `vsrc` operands - the guest's I and J - are
/// ignored; the host interpolates with its own. With equal corners that is unobservable, which
/// is the same reason this test can be exact at all.
///
/// `v_interp_mov_f32` has its own test below.
#[test]
fn a_translated_interpolation_reads_the_interpolated_value() {
    if !device_or_skip("a_translated_interpolation_reads_the_interpolated_value") {
        return;
    }
    let (width, height) = (8, 5);
    let blue = [0.0, 0.0, 1.0, 1.0];
    let clear = [1.0, 0.0, 0.0, 1.0];

    let vertex = interpolated_vertex_module([blue, blue, blue]);
    let by_hand = draw_with(
        &vertex,
        &passthrough_fragment_module(),
        clear,
        width,
        height,
    )
    .expect("the hand-written draw ran");
    let module = translated(Stage::Fragment).expect("the interpolation translated");
    let by_translation =
        draw_with(&vertex, &module, clear, width, height).expect("the translated draw ran");

    assert_eq!(
        by_translation.bytes, by_hand.bytes,
        "the translated interpolation and the hand-written passthrough disagree about the frame"
    );
    for y in 0..height {
        for x in 0..width {
            assert_eq!(
                by_translation.at(x, y),
                Some([0, 0, 255, 255]),
                "pixel ({x}, {y}) - the clear's red here means nothing was interpolated"
            );
        }
    }
}

/// A shader that reads four channels of attribute zero **without interpolating** and exports them.
///
/// `v_interp_mov_f32 vN, <parameter>, attr0.<chan>` four times, then `exp mrt0`. Same encoding
/// as the interpolating one with the family's opcode field - two bits at shift 16 - set to two,
/// and the parameter in the low byte. Both numbers are read off the committed fixture rather
/// than written from a document: `unreached.gcn` holds `v_interp_mov_f32_e32 v4, p0, attr0.x`
/// as `0xc8120002` and `v9, p10, attr31.w` as `0xc8267f00`.
fn parameter_move_shader(parameter: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (register, channel) in (0u32..4).enumerate() {
        let word =
            0xC800_0000u32 | (2 << 16) | ((register as u32) << 18) | (channel << 8) | parameter;
        bytes.extend(word.to_le_bytes());
    }
    bytes.extend(0xF800_000Fu32.to_le_bytes());
    bytes.extend(0x0302_0100u32.to_le_bytes());
    bytes.extend(0xBF81_0000u32.to_le_bytes());
    bytes
}

fn translated_move(parameter: u32, stage: Stage) -> Result<Vec<u32>, TranslateError> {
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");
    let decoded = decode(&parameter_move_shader(parameter), &encodings, &operands);
    translate_for(
        &decoded,
        &encodings,
        Width::Wave64,
        stage,
        orbistoun_translate::wavefront::Window::default(),
    )
    .map(|(module, _)| module)
}

/// The parameter codes, as the committed fixture encodes them.
const P10: u32 = 0;
const P0: u32 = 2;

/// **A translated parameter move reads one corner's value, not a blend of three.**
///
/// The test that distinguishes flat from interpolated, which is the whole content of this
/// translation: the three corners are given *different* colours, so an interpolated read is a
/// gradient and a flat read is one colour. Both are drawn here and compared with each other as
/// well as against the expected bytes, so a failure says which moved.
///
/// The expected colour is the first corner's, which is the provoking vertex under the
/// convention this host uses by default. If a device ever disagreed, this asserts uniformity
/// too - and the two assertions fail differently, which is the point of having both.
#[test]
fn a_translated_parameter_move_reads_one_corner_flat() {
    if !device_or_skip("a_translated_parameter_move_reads_one_corner_flat") {
        return;
    }
    let (width, height) = (8, 5);
    let corners = [
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 1.0],
        [0.0, 0.0, 1.0, 1.0],
    ];
    // Black, which neither corner is, so a draw that produced nothing reads as a different
    // answer rather than as an absent one.
    let clear = [0.0, 0.0, 0.0, 1.0];

    let vertex = interpolated_vertex_module(corners);
    let flat = translated_move(P0, Stage::Fragment).expect("the parameter move translated");
    let framebuffer = draw_with(&vertex, &flat, clear, width, height).expect("the flat draw ran");

    let first = framebuffer.at(0, 0).expect("a pixel");
    for y in 0..height {
        for x in 0..width {
            assert_eq!(
                framebuffer.at(x, y),
                Some(first),
                concat!(
                    "pixel ({}, {}) differs from ({}, {}), so the attribute was interpolated ",
                    "after all"
                ),
                x,
                y,
                0,
                0
            );
        }
    }
    assert_eq!(
        first,
        [255, 0, 0, 255],
        "the flat value should be the first corner's, which is the provoking vertex"
    );

    // The same attribute read the interpolating way is *not* one colour, which is what makes
    // the assertion above a measurement of the decoration rather than of the geometry.
    let smooth = translated(Stage::Fragment).expect("the interpolation translated");
    let gradient =
        draw_with(&vertex, &smooth, clear, width, height).expect("the interpolated draw ran");
    assert!(
        (0..width).any(|x| gradient.at(x, 0) != gradient.at(0, 0)),
        "the interpolated draw is uniform, so this comparison proves nothing"
    );
}

/// **Moving an interpolation delta is refused, not answered with the attribute.**
///
/// `P10` and `P20` are differences between vertices. The host interpolates with its own
/// barycentrics and offers no way to read the gradient it used, so there is nothing to
/// translate - and answering with the attribute's value would be a plausible number that is
/// wrong everywhere.
#[test]
fn moving_an_interpolation_delta_is_refused() {
    assert!(
        translated_move(P10, Stage::Fragment).is_err(),
        "a delta translated, so it read something that is not the delta"
    );
}

/// **An interpolation in a compute dispatch is refused rather than read from nowhere.**
///
/// The control. A compute module has no fragment inputs, so there is nothing to interpolate
/// from - and the failure that matters is not an error but a *read of something arbitrary*.
#[test]
fn an_interpolation_without_a_fragment_input_is_refused() {
    assert!(
        translated(Stage::Compute).is_err(),
        concat!(
            "a compute module translated an interpolation, so it read an attribute that does ",
            "not exist in it"
        )
    );
}
