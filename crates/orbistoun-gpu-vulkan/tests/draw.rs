//! The framebuffer oracle, finished: a draw whose colour reaches the attachment.
//!
//! `attachment.rs` proved the plumbing - clear a colour, copy the image out, read every pixel
//! back. This adds the half that makes it an oracle rather than a pipe: a graphics pipeline, a
//! hand-written vertex shader covering the framebuffer, a hand-written fragment shader writing
//! a constant, and the assertion that the constant is what comes back (D550).
//!
//! # Why the shaders are hand-written
//!
//! This harness exists to check the shader translator, so a shader the translator produced
//! could not check it - the failure would be invisible in exactly the case that matters. The
//! two modules are assembled instruction by instruction through `orbistoun-spirv`, which is a
//! **builder**: it emits the words it is given and verifies that identifiers resolve. That is
//! not the translator, and the distinction is the whole reason this step comes first.
//!
//! It is still project code, so the argument is not that the builder cannot be wrong. It is
//! that a builder wrong enough to matter here would have to be wrong in a way that produces
//! *exactly the colour the test asked for from a different colour*, which is not a failure mode
//! a mistake has.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::{clear_to, draw_with};
use orbistoun_spirv::{constant_colour_fragment_module, fullscreen_triangle_vertex_module};

/// Reports the device, or skips **loudly**.
///
/// A test that finds no device, returns early and reports success would make the suite green on
/// a machine where the only thing it checks never ran.
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

/// **A fragment shader's colour reaches the attachment, and it is not the clear's.**
///
/// # What this asserts, and why the two colours differ
///
/// The attachment is cleared to **red** and the fragment shader writes **blue**. Every pixel
/// comes back blue, which is the whole claim: the shader ran, its output was routed to colour
/// attachment zero, and the copy carried it to the host.
///
/// Making them differ is what turns this from a demonstration into a check. Had both been blue,
/// a pipeline that never bound, a vertex shader that produced no triangle, or a fragment shader
/// whose output went nowhere would all have passed - the clear alone would have produced the
/// expected image. Red is what the failure looks like, and it is a *different* answer rather
/// than an absent one.
///
/// The expectation is written as bytes, `[0, 0, 255, 255]`, beside a request written as floats,
/// `[0.0, 0.0, 1.0, 1.0]`. Two spellings of one fact, so the assertion is not simply the
/// argument read back.
///
/// Every pixel is checked, not a sample. The triangle is deliberately twice the size of the
/// viewport, so a corner left unwritten means the geometry is wrong rather than that the test
/// looked in the wrong place.
///
/// # What it cannot assert
///
/// **That the translator emits anything correct.** Nothing translated is involved. This says
/// the harness can put a shader's output on the screen and read it back, which is what every
/// later comparison rests on - and it is the thing that could not be checked before, because
/// every other part of phase 6 is verified against material this project generated.
///
/// It also says nothing about interpolation, depth, blending or multiple attachments. One
/// constant, one attachment, no depth buffer.
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

/// **The clear-only path still answers the clear, with the pipeline code in place.**
///
/// The refactor that added the draw made `clear_to` a caller of the shared body rather than the
/// body itself. This is the control for the test above: the same machinery, no shaders, and the
/// answer is the colour that was cleared.
///
/// Without it, a pipeline that somehow drew on every path would make the draw test pass for the
/// wrong reason and nothing would notice.
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
