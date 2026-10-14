//! A legacy vertex shader's draw, on a device (D881).
//!
//! With no primitive shader the program neither declares its counts (`MSG_GS_ALLOC_REQ`) nor
//! exports its primitives (`exp prim`): the module does both from the draw, a lane a vertex, the
//! primitives assembled from its topology. A vertex shader that exports only its positions and a
//! parameter draws the triangle its three vertices make.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::draw_mesh_of_vertices_over;
use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
use orbistoun_translate::wavefront::{
    Assembly, GeometryInputs, MeshPrimitive, Stage, UserData, Window,
};
use orbistoun_translate::{Fidelity, Strategy, Width, translate_with_user_data};

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

/// A vertex shader with no geometry-engine traffic: it fetches the position of the vertex whose id
/// is in `v0` from guest memory, exports it, and exports it again as parameter zero.
fn vertex_shader() -> Vec<u8> {
    let words: [u32; 12] = [
        // v14 = v0 * 48, the vertex id the draw seeds there: twelve words a vertex
        0x7e1c_0300,
        0x341e_1c84,
        0x341c_1c85,
        0x4a1c_1d0f,
        // global_load_dwordx4 v[2:5], v[14:15], off
        0xdc38_8000,
        0x027d_000e,
        0xbf8c_3f70,
        // exp pos0 v2..v5 done
        0xf800_08cf,
        0x0504_0302,
        // exp param0 v2..v5, which the passthrough fragment module reads, then s_endpgm
        0xf800_020f,
        0x0504_0302,
        0xbf81_0000,
    ];
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

fn module(legacy: bool) -> Result<Vec<u32>, String> {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let decoded = decode_program(&vertex_shader(), &encodings, &operands);
    translate_with_user_data(
        &decoded,
        &encodings,
        Strategy::Predicated {
            fidelity: Fidelity::Auto,
            width: Width::default(),
        },
        (Stage::Mesh, MeshPrimitive::Triangles),
        Window::default(),
        UserData {
            geometry: Some(GeometryInputs {
                first_vertex: 0,
                vertices: 3,
                primitives: 1,
                assembly: Assembly::List,
                indices: None,
                passthrough: false,
                legacy,
                streamout: false,
            }),
            ..UserData::default()
        },
    )
    .map(|translated| translated.module)
    .map_err(|e| e.to_string())
}

/// A triangle over half the viewport: the corner at (-1, -1) and the two beside it.
fn memory() -> Vec<u32> {
    let corners = [
        [-1.0f32, -1.0, 0.0, 1.0],
        [1.0, -1.0, 0.0, 1.0],
        [-1.0, 1.0, 0.0, 1.0],
    ];
    let mut memory = vec![0u32; 64];
    for (vertex, at) in [0usize, 12, 24].into_iter().enumerate() {
        for component in 0..4 {
            memory[at + component] = corners[vertex][component].to_bits();
        }
    }
    memory
}

/// The legacy draw's module declares its vertices, seeds each one's id in `v0` and assembles their
/// triangle itself: the half of the viewport its three vertices bound is drawn, and the other half
/// is not - which holds only where each vertex fetched its own position.
#[test]
fn a_legacy_vertex_shader_draws_the_primitives_its_draw_assembles() {
    if !device_or_skip("a_legacy_vertex_shader_draws_the_primitives_its_draw_assembles") {
        return;
    }
    let fragment = orbistoun_spirv::passthrough_fragment_module();
    let legacy = module(true).expect("the legacy vertex shader translates");
    // The draw's three vertices, the count its words carry (D745).
    let (pixels, _) = draw_mesh_of_vertices_over((&legacy, &fragment), 3, (8, 8), &memory())
        .expect("the draw ran");
    // The target starts clear, and a drawn pixel is opaque.
    let drawn = |x, y| pixels.at(x, y).expect("inside")[3] == 255;
    // Inside the half its vertices bound, near each of its corners; and past its long edge.
    for (x, y) in [(1, 1), (5, 1), (1, 5)] {
        assert!(drawn(x, y), "({x}, {y}) is inside the triangle");
    }
    for (x, y) in [(6, 6), (7, 7), (4, 5)] {
        assert!(!drawn(x, y), "({x}, {y}) is past its long edge");
    }
}
