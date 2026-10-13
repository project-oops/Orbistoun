//! The samples a counting draw passes, as a `ZPASS_DONE` reads them (D827).

use orbistoun_gpu::{
    RenderBackend, RenderCommand, Resource, ResourceId, ShaderStage, TargetEncoding,
};
use orbistoun_gpu_vulkan::VulkanBackend;
use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_spirv::{constant_colour_fragment_module, fullscreen_triangle_vertex_module};

/// A full-screen draw over an 8x4 target passes its 32 samples while counting is on, and adds
/// nothing while it is off: the count a z-pass counter dump reads is the pixels the depth test
/// passed, as the console's backends sum to (obSCEne `-4d19`).
#[test]
fn a_counting_draw_adds_the_samples_it_passed() {
    match probe() {
        Availability::Available { properties } => println!("device: {}", properties.device),
        Availability::Unavailable { reason } => {
            println!("SKIPPED - no device: {reason}");
            return;
        }
    }
    let vertex = fullscreen_triangle_vertex_module();
    let fragment = constant_colour_fragment_module([0.0, 0.0, 1.0, 1.0]);
    let mut backend = VulkanBackend::new();
    backend
        .ensure_resident(ResourceId(1), Resource::Shader(&vertex))
        .expect("vertex");
    backend
        .ensure_resident(ResourceId(2), Resource::Shader(&fragment))
        .expect("fragment");
    backend
        .ensure_resident(
            ResourceId(10),
            Resource::RenderTarget {
                width: 8,
                height: 4,
                encoding: TargetEncoding::Unorm8,
            },
        )
        .expect("target");
    for command in [
        RenderCommand::SetRenderTargets {
            colour: vec![ResourceId(10)],
            depth: None,
        },
        RenderCommand::BindShader {
            stage: ShaderStage::Vertex,
            shader: ResourceId(1),
        },
        RenderCommand::BindShader {
            stage: ShaderStage::Fragment,
            shader: ResourceId(2),
        },
    ] {
        backend.execute(&command).expect("set up");
    }
    let Some(before) = backend.samples_passed() else {
        println!("SKIPPED - the device does not count samples exactly");
        return;
    };
    let draw = RenderCommand::Draw {
        vertices: 3,
        instances: 1,
        first_vertex: 0,
    };
    backend
        .execute(&RenderCommand::SetSampleCounting(true))
        .expect("counting");
    backend.execute(&draw).expect("a counted draw");
    let counted = backend.samples_passed().expect("a count");
    assert_eq!(counted - before, 32, "every pixel of the target passed");
    backend
        .execute(&RenderCommand::SetSampleCounting(false))
        .expect("not counting");
    backend.execute(&draw).expect("an uncounted draw");
    assert_eq!(backend.samples_passed(), Some(counted), "nothing counted");
}
