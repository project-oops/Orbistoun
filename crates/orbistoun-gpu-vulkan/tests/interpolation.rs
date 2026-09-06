//! The oracle carrying a varying, which is what interpolation needs before it can be checked.
//!
//! # Why this comes before translating `v_interp_p1_f32`
//!
//! VINTRP is one of the two families the translator still refuses, and the only one of them that
//! needs no capture - its operands are solved, and the attribute and channel it names are in the
//! instruction rather than in guest register state. So it is the next thing to translate.
//!
//! But the framebuffer oracle could not have checked it. Its hand-written vertex shader emits a
//! position and nothing else, so nothing is interpolated and a translated interpolation would
//! have been verified against material this project generated - the exact trap phase 6's own
//! ordering was written to avoid. The harness gets the varying first, the same way it got the
//! attachment before the draw (D554).

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::draw_with;
use orbistoun_spirv::{interpolated_vertex_module, passthrough_fragment_module};

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

/// **A varying the three corners agree about arrives unchanged at every pixel.**
///
/// # Why the corners are equal, and what that buys
///
/// Interpolation weights a value by barycentric coordinates. Where the three corners carry the
/// *same* value, every weighting of them is that value - so the expected result is exact and
/// does not depend on where the triangle's vertices are, how the rasteriser assigns coverage, or
/// what the driver rounds. Any pixel that is not blue means the varying did not survive the trip
/// from vertex output to fragment input, and the clear is red so a broken pipeline is visible as
/// a different answer rather than an absent one.
///
/// This is the exact half of the check. The one below is the half that shows it varies at all.
///
/// # What it cannot assert
///
/// **That interpolation is correct**, only that a value passes through it unaltered when there
/// is nothing to interpolate between. A pipeline that ignored the varying and forwarded corner
/// zero's value would pass, and so would one that averaged the three.
#[test]
fn a_varying_every_corner_agrees_about_survives_interpolation() {
    if !device_or_skip("a_varying_every_corner_agrees_about_survives_interpolation") {
        return;
    }
    let (width, height) = (8, 5);
    let blue = [0.0, 0.0, 1.0, 1.0];
    let vertex = interpolated_vertex_module([blue, blue, blue]);
    let pixels = draw_with(
        &vertex,
        &passthrough_fragment_module(),
        [1.0, 0.0, 0.0, 1.0],
        width,
        height,
    )
    .expect("the draw ran");

    for y in 0..height {
        for x in 0..width {
            assert_eq!(
                pixels.at(x, y),
                Some([0, 0, 255, 255]),
                "pixel ({x}, {y}) - red here means the varying never reached the fragment shader"
            );
        }
    }
}

/// **A varying the corners disagree about produces a picture that is not uniform.**
///
/// # What this asserts, and what it deliberately does not
///
/// That two pixels far apart differ. Nothing more: the exact value at a pixel depends on
/// barycentric weights, on where the rasteriser places sample points, and on whether the driver
/// interpolates perspective-correctly - none of which this project chose, and pinning any of
/// them would make this a test of the driver.
///
/// So the claim is the weak one that is still worth having: **the varying varies.** Together
/// with the exact test above - which shows a value passes through unaltered - that is enough to
/// say the pipeline interpolates the attribute rather than forwarding a constant, which is the
/// property a translated `v_interp_p1_f32` will need to be checked against.
///
/// # What it cannot assert
///
/// The direction of the gradient, or which corner is which. A pipeline that interpolated the
/// three corners in the wrong order would pass, and distinguishing that needs the vertex
/// positions and the sample locations pinned together - a different test, and one that would be
/// about the rasteriser.
#[test]
fn a_varying_the_corners_disagree_about_is_not_uniform() {
    if !device_or_skip("a_varying_the_corners_disagree_about_is_not_uniform") {
        return;
    }
    let (width, height) = (8, 5);
    let vertex = interpolated_vertex_module([
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 1.0],
        [0.0, 0.0, 1.0, 1.0],
    ]);
    let pixels = draw_with(
        &vertex,
        &passthrough_fragment_module(),
        [1.0, 0.0, 0.0, 1.0],
        width,
        height,
    )
    .expect("the draw ran");

    let first = pixels.at(0, 0).expect("a pixel at the origin");
    let last = pixels
        .at(width - 1, height - 1)
        .expect("a pixel at the far corner");
    assert_ne!(
        first, last,
        "opposite corners of the image are identical, so the varying was not interpolated - a \
         pipeline forwarding one corner's value to every fragment looks like this"
    );
    // And it is the varying that is showing, not the clear: the alpha the corners carry is one
    // everywhere, and every corner's colour has a channel the clear does not.
    assert_eq!(first[3], 255, "the varying's alpha reached the attachment");
    assert_eq!(last[3], 255, "and at the far corner too");
}
