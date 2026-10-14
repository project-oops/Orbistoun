//! A primitive shader that exports clip distances, on a device (D867).
//!
//! The draw's `PA_CL_VS_OUT_CNTL` says what `pos1` to `pos3` are: with no misc vector and the first
//! distance vector enabled, `pos1` carries distances 0-3, and `CLIP_DIST_ENA_n` says which of them
//! clip. A primitive whose vertices all lie outside an enabled distance is not drawn.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::draw_mesh_over;
use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
use orbistoun_translate::wavefront::{MeshPrimitive, Stage, UserData, Window};
use orbistoun_translate::{Fidelity, Strategy, Width, translate_with_user_data};

/// `VS_OUT_CCDIST0_VEC_ENA`: distances 0-3 are exported, here in `pos1`.
const CCDIST0: u32 = 1 << 22;
/// `CLIP_DIST_ENA_0`: distance 0 clips.
const CLIP_0: u32 = 1;

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

/// A primitive shader declaring three vertices and one triangle, fetching each lane's position from
/// guest memory, exporting the triangle, the positions, `distance` as every vertex's first exported
/// distance - `exp pos1 v20, off, off, off` (`0xf80000d1`) - and the positions again as parameter
/// zero.
fn shader(distance: f32) -> Vec<u8> {
    let words = [
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
        // v20 = distance; exp pos1 v20, off, off, off
        0x7e28_02ff,
        distance.to_bits(),
        0xf800_00d1,
        20,
        // exp param0 v2..v5, which the passthrough fragment module reads, then s_endpgm
        0xf800_020f,
        0x0504_0302,
        0xbf81_0000,
    ];
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// Whether a full-screen triangle whose vertices export `distance` is drawn under `routing`.
fn drawn(distance: f32, routing: u32) -> bool {
    let encodings = EncodingTable::builtin().expect("encodings");
    let operands = OperandTable::builtin().expect("operands");
    let decoded = decode_program(&shader(distance), &encodings, &operands);
    let module = translate_with_user_data(
        &decoded,
        &encodings,
        Strategy::Predicated {
            fidelity: Fidelity::Auto,
            width: Width::default(),
        },
        (Stage::Mesh, MeshPrimitive::Triangles),
        Window::default(),
        UserData {
            position_vectors: Some(routing),
            ..UserData::default()
        },
    )
    .expect("the clipping primitive shader translates")
    .module;
    let corners = [
        [-1.0f32, -1.0, 0.0, 1.0],
        [3.0, -1.0, 0.0, 1.0],
        [-1.0, 3.0, 0.0, 1.0],
    ];
    let mut memory = vec![0u32; 64];
    for (vertex, at) in [0usize, 12, 24].into_iter().enumerate() {
        for component in 0..4 {
            memory[at + component] = corners[vertex][component].to_bits();
        }
    }
    let fragment = orbistoun_spirv::passthrough_fragment_module();
    let (pixels, _) = draw_mesh_over(&module, &fragment, [0.0, 0.0, 1.0, 1.0], 8, 6, &memory)
        .expect("the draw ran");
    pixels.at(4, 3).expect("inside") != [0, 0, 255, 255]
}

/// An exported distance the draw clips by keeps a primitive inside it and drops one outside; one it
/// does not clip by drops nothing.
#[test]
fn an_exported_clip_distance_clips_where_the_draw_enables_it() {
    if !device_or_skip("an_exported_clip_distance_clips_where_the_draw_enables_it") {
        return;
    }
    assert!(drawn(1.0, CCDIST0 | CLIP_0), "inside distance 0");
    assert!(!drawn(-1.0, CCDIST0 | CLIP_0), "outside distance 0");
    assert!(
        drawn(-1.0, CCDIST0),
        "distance 0 exported but not clipped by"
    );
}
