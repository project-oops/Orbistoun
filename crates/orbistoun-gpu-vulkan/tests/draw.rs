//! The framebuffer oracle: a draw whose colour reaches the attachment (D549).
//!
//! `attachment.rs` checks the plumbing; this adds a graphics pipeline, a hand-written vertex shader
//! covering the framebuffer and a hand-written fragment shader writing a constant, and asserts the
//! constant comes back. The shaders are assembled through `orbistoun-spirv`, a builder that emits
//! the words it is given, not the translator this harness exists to check.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::{clear_to, draw_with};
use orbistoun_spirv::{constant_colour_fragment_module, fullscreen_triangle_vertex_module};

/// Reports the device, or skips loudly: a test that returns early and reports success would make
/// the suite green where nothing ran.
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

/// A fragment shader's colour reaches the attachment, and it is not the clear's.
///
/// The attachment is cleared to red and the fragment shader writes blue, so a pipeline that never
/// bound or a shader whose output went nowhere reads as a different answer. The expectation is
/// bytes, `[0, 0, 255, 255]`, beside a request in floats, so the assertion is not the argument read
/// back. Every pixel is checked, and the triangle is twice the viewport, so an unwritten corner
/// means wrong geometry. One constant, one attachment, no depth, no interpolation, and nothing
/// translated.
#[test]
fn a_fragment_shader_writes_its_colour_into_the_attachment() {
    if !device_or_skip("a_fragment_shader_writes_its_colour_into_the_attachment") {
        return;
    }

    let (width, height) = (8, 5);
    let blue = [0.0, 0.0, 1.0, 1.0];
    let red = [1.0, 0.0, 0.0, 1.0];

    let vertex = fullscreen_triangle_vertex_module();
    let fragment = constant_colour_fragment_module(blue);
    let pixels = draw_with(&vertex, &fragment, red, width, height).expect("the draw ran");

    assert_eq!(pixels.width, width);
    assert_eq!(pixels.height, height);
    for y in 0..height {
        for x in 0..width {
            assert_eq!(
                pixels.at(x, y),
                Some([0, 0, 255, 255]),
                concat!(
                    "pixel ({}, {}) is not the fragment shader's blue - red would mean the ",
                    "clear survived and the draw did nothing"
                ),
                x,
                y
            );
        }
    }
}

/// The clear-only path still answers the clear, with the pipeline code in place: the control for
/// the test above, so a pipeline that drew on every path cannot pass it for the wrong reason.
#[test]
fn drawing_nothing_still_answers_the_clear() {
    if !device_or_skip("drawing_nothing_still_answers_the_clear") {
        return;
    }
    let pixels = clear_to([1.0, 0.0, 0.0, 1.0], 8, 5).expect("the clear path ran");
    assert_eq!(
        pixels.at(0, 0),
        Some([255, 0, 0, 255]),
        "with no shaders the attachment holds what it was cleared to"
    );
}
