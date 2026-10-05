//! Compiled pipelines are kept where the host names (D744): a draw's pipeline is written there as
//! a pipeline cache this device made. Its own test binary, because the place is the process's and
//! must be named before the device opens.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::draw_with_texture;
use orbistoun_gpu_vulkan::pipeline_cache::keep_pipelines_at;
use orbistoun_spirv::{Lod, interpolated_vertex_module, sampling_fragment_module};

#[test]
fn a_compiled_pipeline_is_kept_where_the_host_names() {
    let path = std::env::temp_dir().join(format!(
        "orbistoun-kept-pipelines-{}.bin",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    keep_pipelines_at(path.clone());
    match probe() {
        Availability::Available { properties } => println!("device: {}", properties.device),
        Availability::Unavailable { reason } => {
            println!("SKIPPED - no device: {reason}");
            return;
        }
    }
    let vertex = interpolated_vertex_module([
        [0.0, 0.0, 0.0, 1.0],
        [2.0, 0.0, 0.0, 1.0],
        [0.0, 2.0, 0.0, 1.0],
    ]);
    draw_with_texture(
        &vertex,
        &sampling_fragment_module(Lod::Zero),
        [0.0; 4],
        (4, 4),
        (&[u32::MAX; 4], 2),
    )
    .expect("the draw ran");
    let kept = std::fs::read(&path).expect("the pipeline was kept");
    let _ = std::fs::remove_file(&path);
    // A version-one header (its own length 32, version 1), then the driver's data.
    assert_eq!(kept.get(..8), Some(&[32, 0, 0, 0, 1, 0, 0, 0][..]));
    assert!(kept.len() > 32, "the cache holds more than its header");
}
