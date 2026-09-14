//! A mesh stage on a device, which is where a guest's primitive shader has to end up.
//!
//! D688 decided that an NGG primitive shader translates to a mesh shader: it declares how many
//! vertices and primitives it will emit, writes the primitive's indices, and writes per-vertex
//! outputs, which is `MSG_GS_ALLOC_REQ`, `exp prim` and `exp pos`/`exp param` in that order.
//! Nothing had run. This is the host half - hand-assembled, like every other oracle here -
//! standing up before anything is translated into it, which is the order D549 argues for and
//! the order the fragment work followed.
//!
//! The comparison is the point. The same triangle drawn by the vertex path and by the mesh
//! path must produce the same frame, because the two differ only in which stage produced the
//! geometry.

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

/// **A mesh shader draws, and draws what the vertex path draws.**
///
/// # What it asserts
///
/// Every pixel is the colour the mesh shader gave its vertices, over a clear it never writes -
/// so a draw that produced nothing reads as a different answer rather than an absent one. And
/// the frame is compared byte for byte against the same triangle drawn through the vertex
/// stage, which is the assertion that means something: the two paths share a fragment shader
/// and an attachment and differ only in what produced the geometry.
///
/// # What it cannot assert
///
/// That a *translated* primitive shader would do this. Nothing is translated here - the mesh
/// module is hand-assembled, exactly like the vertex module it is compared against, and the
/// point is to have an oracle before there is anything to check against it.
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
