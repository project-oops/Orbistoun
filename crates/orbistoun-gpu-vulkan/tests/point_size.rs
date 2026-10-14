//! A primitive shader that exports a point size, on a device (D867).
//!
//! `exp pos1` with its first channel names each vertex's point size, a float, which the point is
//! drawn at: radeonsi writes it whenever the shader writes `gl_PointSize`, and sets
//! `PA_CL_VS_OUT_CNTL.USE_VTX_POINT_SIZE` so the hardware reads it.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::draw_mesh_over;
use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
use orbistoun_translate::wavefront::{MeshPrimitive, Stage, Window};
use orbistoun_translate::{Fidelity, Strategy, Width, translate_windowed_primitive};

fn device_or_skip(what: &str) -> bool {
    match probe() {
        Availability::Available { properties } if properties.mesh_shading => {
            println!("[{what}] device: {}", properties.device);
            true
        }
        Availability::Available { properties } => {
            println!("[{what}] SKIPPED - {} has no mesh stage", properties.device);
            false
        }
        Availability::Unavailable { reason } => {
            println!("[{what}] SKIPPED - no device: {reason}");
            false
        }
    }
}

/// A primitive shader declaring one vertex and one point, fetching the vertex's position from
/// guest memory, exporting the point `(0)`, the position, `size` as the vertex's point size -
/// `exp pos1 v20, off, off, off`, the form GCTS00001's clip-distance shaders take
/// (`0xf80000d1`) - and the position again as parameter zero.
fn sized_point(size: f32) -> Vec<u8> {
    let words = [
        // m0 = one vertex, one primitive; s_sendmsg sendmsg(MSG_GS_ALLOC_REQ)
        0xbefc_03ff,
        0x0000_1001,
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
        // v1 = vertex 0; exp prim
        0x7e02_0280,
        0xf800_0941,
        0x0000_0001,
        // exp pos0 v2..v5
        0xf800_08cf,
        0x0504_0302,
        // v20 = size; exp pos1 v20, off, off, off
        0x7e28_02ff,
        size.to_bits(),
        0xf800_00d1,
        20,
        // exp param0 v2..v5, which the passthrough fragment module reads, then s_endpgm
        0xf800_020f,
        0x0504_0302,
        0xbf81_0000,
    ];
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

fn module(size: f32) -> Vec<u32> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let decoded = decode_program(&sized_point(size), &encodings, &operands);
    translate_windowed_primitive(
        &decoded,
        &encodings,
        Strategy::Predicated {
            fidelity: Fidelity::Auto,
            width: Width::default(),
        },
        Stage::Mesh,
        MeshPrimitive::Points,
        Window::default(),
    )
    .expect("the sized point shader translates")
    .module
}

/// How many pixels a point at the centre of a 16x16 target covers when its vertex exports `size`.
fn covered(size: f32) -> usize {
    let mut memory = vec![0u32; 64];
    for (component, value) in [0.0f32, 0.0, 0.0, 1.0].into_iter().enumerate() {
        memory[component] = value.to_bits();
    }
    let fragment = orbistoun_spirv::passthrough_fragment_module();
    let clear = [0.0, 0.0, 1.0, 1.0];
    let (pixels, _) =
        draw_mesh_over(&module(size), &fragment, clear, 16, 16, &memory).expect("the draw ran");
    (0..16u32)
        .flat_map(|y| (0..16u32).map(move |x| (x, y)))
        .filter(|&(x, y)| pixels.at(x, y).expect("inside") != [0, 0, 255, 255])
        .count()
}

/// A point is drawn at the size its vertex exports: a pixel at one, a four-by-four square at four.
#[test]
fn a_point_is_drawn_at_the_size_its_vertex_exports() {
    if !device_or_skip("a_point_is_drawn_at_the_size_its_vertex_exports") {
        return;
    }
    assert_eq!(covered(1.0), 1, "a point of size one is one pixel");
    assert_eq!(covered(4.0), 16, "a point of size four is four by four");
}
