//! A mesh stage on a device, where a guest's primitive shader is translated to (D688).
//!
//! A mesh shader declares how many vertices and primitives it emits, writes the primitive's
//! indices, and writes per-vertex outputs: `MSG_GS_ALLOC_REQ`, `exp prim` and `exp pos`/`exp param`
//! in that order. This is the hand-assembled host half. The same triangle drawn by the vertex path
//! and the mesh path must produce the same frame, since they differ only in which stage produced
//! the geometry.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::{draw_mesh_with, draw_with};
use orbistoun_spirv::{passthrough_fragment_module, triangle_mesh_module};

/// Whether the device can run a mesh stage at all, said aloud when it cannot.
fn mesh_or_skip(what: &str) -> bool {
    match probe() {
        Availability::Available { properties } if properties.mesh_shading => {
            println!("[{what}] device: {}", properties.device);
            true
        }
        Availability::Available { properties } => {
            println!(
                "[{what}] SKIPPED - {} has no mesh stage, so a guest's geometry cannot run here",
                properties.device
            );
            false
        }
        Availability::Unavailable { reason } => {
            println!("[{what}] SKIPPED - no device: {reason}");
            false
        }
    }
}

/// A mesh shader draws, and draws what the vertex path draws.
///
/// Every pixel is the mesh vertices' colour, over a clear it never writes, and the frame matches
/// the same triangle drawn through the vertex stage byte for byte: the two share a fragment shader
/// and an attachment. Nothing is translated.
#[test]
fn a_mesh_shader_draws_the_triangle_the_vertex_path_draws() {
    if !mesh_or_skip("a_mesh_shader_draws_the_triangle_the_vertex_path_draws") {
        return;
    }
    let (width, height) = (8, 5);
    let green = [0.0, 1.0, 0.0, 1.0];
    let red = [1.0, 0.0, 0.0, 1.0];

    let mesh = triangle_mesh_module([green; 3]);
    let by_mesh = draw_mesh_with(&mesh, &passthrough_fragment_module(), red, width, height)
        .expect("the mesh draw ran");

    for y in 0..height {
        for x in 0..width {
            assert_eq!(
                by_mesh.at(x, y),
                Some([0, 255, 0, 255]),
                "pixel ({x}, {y}) - red here is the clear, so the mesh stage emitted nothing"
            );
        }
    }

    let vertex = orbistoun_spirv::interpolated_vertex_module([green; 3]);
    let by_vertex = draw_with(&vertex, &passthrough_fragment_module(), red, width, height)
        .expect("the vertex draw ran");
    assert_eq!(
        by_mesh.bytes, by_vertex.bytes,
        "the mesh and vertex paths disagree about a triangle they both cover the viewport with"
    );
}
