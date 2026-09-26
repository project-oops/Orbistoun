//! A translated pixel shader's system values, checked against the framebuffer.
//!
//! The guest finds its pixel position in the vector registers after its barycentrics, placed by
//! `SPI_PS_INPUT_ADDR`. The shader here scales `POS_X_FLOAT` and `POS_Y_FLOAT` into its red and
//! green, so every pixel's colour names the coordinates it was drawn at.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::draw_with;
use orbistoun_shader::{EncodingTable, OperandTable, decode};
use orbistoun_spirv::interpolated_vertex_module;
use orbistoun_translate::Width;
use orbistoun_translate::wavefront::{
    MeshPrimitive, PixelInputs, Stage, UserData, Window, translate_with_user_data,
};

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

/// `PERSP_CENTER_ENA`, `POS_X_FLOAT_ENA` and `POS_Y_FLOAT_ENA` (`gfx103.json`): the barycentric
/// pair takes `v0`-`v1`, so the position lands in `v2` and `v3`.
const CENTRE_AND_POSITION: u32 = 0x302;

/// The factor each coordinate is scaled by, as `f32` bits: an eighth keeps an 8-wide target's
/// pixel centres inside the unit range a `UNORM` attachment holds.
const EIGHTH: u32 = 0x3e00_0000;

/// The source code for a literal dword following the instruction.
const LITERAL: u32 = 0xff;
/// The source code for inline integer 0.
const INLINE_ZERO: u32 = 0x80;
/// The source code for inline 1.0.
const INLINE_ONE: u32 = 0xf2;

/// `v_mov_b32_e32 vDST, <constant>`: VOP1 opcode 1.
fn mov(dst: u32, constant: u32) -> u32 {
    0x7E00_0000 | (dst << 17) | (1 << 9) | constant
}

/// A shader that writes `(x / 8, y / 8, 0, 1)` from its position registers.
///
/// `v_mul_f32_e32` (VOP2 opcode 8) with a literal source 0 scales `v2` into `v0` and `v3` into
/// `v1`; two moves write 0 and 1.0 into `v4` and `v5`; then `exp mrt0 v0, v1, v4, v5` and
/// `s_endpgm`. Field positions are those `data/encodings.toml` records.
fn position_shader() -> Vec<u8> {
    let multiply = |dst: u32, src: u32| [(8 << 25) | (dst << 17) | (src << 9) | LITERAL, EIGHTH];
    let words = multiply(0, 2).into_iter().chain(multiply(1, 3)).chain([
        mov(4, INLINE_ZERO),
        mov(5, INLINE_ONE),
        0xF800_000F,
        0x0504_0100,
        0xBF81_0000,
    ]);
    words.flat_map(u32::to_le_bytes).collect()
}

fn translated(pixel_inputs: Option<PixelInputs>) -> Vec<u32> {
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");
    let decoded = decode(&position_shader(), &encodings, &operands);
    translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Fragment, MeshPrimitive::default()),
        Window::default(),
        UserData {
            pixel_inputs,
            ..UserData::default()
        },
    )
    .expect("the position shader translated")
    .0
}

/// The byte a `UNORM` attachment stores for a pixel centre scaled by an eighth.
fn expected(coordinate: u32) -> u8 {
    let value = (f64::from(coordinate) + 0.5) / 8.0 * 255.0;
    // At most 255 by the target's size, so the cast keeps the value.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = value.round() as u8;
    byte
}

/// Each pixel reads its own centre from `POS_X_FLOAT` and `POS_Y_FLOAT`.
///
/// The expected red and green are computed from each pixel's coordinates, and differ in every
/// column and row, so a position read as zero or as one pixel's value everywhere fails. One step
/// of tolerance allows the conversion's rounding.
#[test]
fn a_translated_pixel_shader_reads_its_position() {
    if !device_or_skip("a_translated_pixel_shader_reads_its_position") {
        return;
    }
    let (width, height) = (8, 5);
    let vertex = interpolated_vertex_module([[0.0; 4]; 3]);
    let module = translated(Some(PixelInputs {
        enable: CENTRE_AND_POSITION,
        address: CENTRE_AND_POSITION,
    }));
    let drawn =
        draw_with(&vertex, &module, [1.0, 1.0, 1.0, 1.0], width, height).expect("the draw ran");
    for y in 0..height {
        for x in 0..width {
            let pixel = drawn.at(x, y).expect("a pixel");
            let near = |got: u8, want: u8| got.abs_diff(want) <= 1;
            assert!(
                near(pixel[0], expected(x)) && near(pixel[1], expected(y)) && pixel[2] == 0,
                "pixel ({x}, {y}) is {pixel:?}, expected red {} and green {}",
                expected(x),
                expected(y)
            );
        }
    }
}

/// Every seeded field together: the pair, position, facing and the ancillary word, `v2`-`v7`.
const EVERY_SYSTEM_VALUE: u32 = 0x3f02;

/// A shader that writes `(facing, ancillary, ancillary, 1)`: `v_mov_b32 v8, 1.0`, then
/// `exp mrt0 v6, v7, v7, v8` and `s_endpgm`.
fn facing_and_layer_shader() -> Vec<u8> {
    [mov(8, INLINE_ONE), 0xF800_000F, 0x0807_0706, 0xBF81_0000]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect()
}

/// A module reading facing and the layer is accepted by the device and reads what a single-layer
/// draw has: facing is `+1.0` or `-1.0`, one value for the whole triangle, which a `UNORM` target
/// stores as 255 or 0; the ancillary word is zero, since layer 0 is the only one.
#[test]
fn facing_is_a_signed_one_and_a_single_layer_reads_zero() {
    if !device_or_skip("facing_is_a_signed_one_and_a_single_layer_reads_zero") {
        return;
    }
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");
    let decoded = decode(&facing_and_layer_shader(), &encodings, &operands);
    let module = translate_with_user_data(
        &decoded,
        &encodings,
        Width::Wave64,
        (Stage::Fragment, MeshPrimitive::default()),
        Window::default(),
        UserData {
            pixel_inputs: Some(PixelInputs {
                enable: EVERY_SYSTEM_VALUE,
                address: EVERY_SYSTEM_VALUE,
            }),
            ..UserData::default()
        },
    )
    .expect("the facing shader translated")
    .0;
    let vertex = interpolated_vertex_module([[0.0; 4]; 3]);
    let drawn = draw_with(&vertex, &module, [0.5, 0.5, 0.5, 0.5], 8, 5).expect("the draw ran");
    let first = drawn.at(0, 0).expect("a pixel");
    assert!(
        first == [255, 0, 0, 255] || first == [0, 0, 0, 255],
        "got {first:?}"
    );
    for y in 0..5 {
        for x in 0..8 {
            assert_eq!(drawn.at(x, y), Some(first), "pixel ({x}, {y})");
        }
    }
}

/// Without an input layout nothing is seeded, so the same shader reads zeros: the check above
/// measures the seeding rather than something the shader computes on its own.
#[test]
fn without_a_layout_the_position_registers_read_zero() {
    if !device_or_skip("without_a_layout_the_position_registers_read_zero") {
        return;
    }
    let vertex = interpolated_vertex_module([[0.0; 4]; 3]);
    let drawn =
        draw_with(&vertex, &translated(None), [1.0, 1.0, 1.0, 1.0], 8, 5).expect("the draw ran");
    assert_eq!(drawn.at(7, 4), Some([0, 0, 0, 255]));
}
