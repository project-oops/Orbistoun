//! A primitive shader emitting a rectangle list, on a device.
//!
//! radeonsi's blits set `VGT_GS_OUT_PRIM_TYPE` to `RECTLIST` (`si_pipe.h:2110`) and give each
//! rectangle three corners - `(x1, y1)`, `(x1, y2)`, `(x2, y1)`, vertex ids 0, 1, 2
//! (`si_nir_lower_vs_inputs.c:97-122`) - so the rectangle is the parallelogram those three span, its
//! fourth corner `v1 + v2 - v0`. The mesh module draws it as two triangles over that fourth vertex,
//! with every parameter carried the same way, so the parameters stay one plane across the whole
//! rectangle.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::draw_mesh_over;
use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
use orbistoun_translate::wavefront::{MeshPrimitive, Stage, Window};
use orbistoun_translate::{Fidelity, Strategy, Width, translate_windowed_primitive};

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

/// A primitive shader declaring three vertices and one primitive, fetching each lane's vertex from
/// guest memory, and exporting the primitive `(0, 1, 2)`, the positions, and the positions again as
/// parameter zero.
///
/// The instruction words are those of `console_fragment.rs`'s minimal primitive shader: the export
/// words are oracle record A's.
fn primitive_shader() -> Vec<u8> {
    let words: [u32; 17] = [
        // m0 = three vertices, one primitive; s_sendmsg sendmsg(MSG_GS_ALLOC_REQ)
        0xbefc_03ff,
        0x0000_1003,
        0xbf90_0009,
        // v14 = lane * 48: twelve words a vertex
        0xd765_000e,
        0x0001_00c1,
        0x341e_1c84,
        0x341c_1c85,
        0x4a1c_1d0f,
        // global_load_dwordx4 v[2:5], v[14:15], off
        0xdc38_8000,
        0x027d_000e,
        0xbf8c_3f70,
        // v1 = vertices 0, 1, 2 with edge flags; exp prim
        0x7e02_02ff,
        0x2028_0600,
        0xf800_0941,
        0x0000_0001,
        // exp pos0 v2..v5
        0xf800_08cf,
        0x0504_0302,
    ];
    let mut bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    // exp param0 v2..v5, then s_endpgm
    for word in [0xf800_020f_u32, 0x0504_0302, 0xbf81_0000] {
        bytes.extend(word.to_le_bytes());
    }
    bytes
}

fn mesh_module(primitive: MeshPrimitive) -> Vec<u32> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let decoded = decode_program(&primitive_shader(), &encodings, &operands);
    translate_windowed_primitive(
        &decoded,
        &encodings,
        Strategy::Predicated {
            fidelity: Fidelity::Auto,
            width: Width::default(),
        },
        Stage::Mesh,
        primitive,
        Window::default(),
    )
    .expect("the primitive shader translates")
    .module
}

/// Three corners of the whole viewport in radeonsi's order: `(x1, y1)`, `(x1, y2)`, `(x2, y1)`.
fn memory() -> Vec<u32> {
    let corners = [
        [-1.0f32, -1.0, 0.0, 1.0],
        [-1.0, 1.0, 0.0, 1.0],
        [1.0, -1.0, 0.0, 1.0],
    ];
    let mut memory = vec![0u32; 64];
    for (vertex, at) in [0usize, 12, 24].into_iter().enumerate() {
        for component in 0..4 {
            memory[at + component] = corners[vertex][component].to_bits();
        }
    }
    memory
}

/// The rectangle covers every pixel, and parameter zero - the position - is one plane across it:
/// at each pixel centre it is that pixel's own clip-space position, clamped to the target's unorm
/// range. As a triangle list the same three vertices leave the far corner at the clear colour.
#[test]
fn a_rectangle_list_covers_the_parallelogram_its_three_corners_span() {
    if !device_or_skip("a_rectangle_list_covers_the_parallelogram_its_three_corners_span") {
        return;
    }
    let (width, height) = (8u16, 6u16);
    let fragment = orbistoun_spirv::passthrough_fragment_module();
    let clear = [0.0, 0.0, 1.0, 1.0];

    let (pixels, _) = draw_mesh_over(
        &mesh_module(MeshPrimitive::Rectangles),
        &fragment,
        clear,
        u32::from(width),
        u32::from(height),
        &memory(),
    )
    .expect("the rectangle draw ran");
    let unorm = |ndc: f32| ndc.clamp(0.0, 1.0) * 255.0;
    for y in 0..height {
        for x in 0..width {
            let ndc_x = (f32::from(x) + 0.5) * 2.0 / f32::from(width) - 1.0;
            let ndc_y = (f32::from(y) + 0.5) * 2.0 / f32::from(height) - 1.0;
            let got = pixels.at(u32::from(x), u32::from(y)).expect("inside");
            let expected = [unorm(ndc_x), unorm(ndc_y), 0.0, 255.0];
            for (channel, (&g, e)) in got.iter().zip(expected).enumerate() {
                assert!(
                    (f32::from(g) - e).abs() <= 2.0,
                    "pixel ({x}, {y}) channel {channel}: {got:?}, expected {expected:?}"
                );
            }
        }
    }

    let (triangle, _) = draw_mesh_over(
        &mesh_module(MeshPrimitive::Triangles),
        &fragment,
        clear,
        u32::from(width),
        u32::from(height),
        &memory(),
    )
    .expect("the triangle draw ran");
    assert_eq!(
        triangle.at(u32::from(width) - 1, u32::from(height) - 1),
        Some([0, 0, 255, 255]),
        "a triangle leaves the fourth corner clear"
    );
}
