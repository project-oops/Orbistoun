//! The oracle carrying a varying, which a translated `v_interp_p1_f32` is checked against.
//!
//! The framebuffer oracle's vertex shader emits only a position, so nothing is interpolated. This
//! harness gets the varying first, hand-assembled, so a translated interpolation is not verified
//! against material the translator produced (D549).

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

/// A varying the three corners agree about arrives unchanged at every pixel.
///
/// Where the corners carry the same value, every barycentric weighting is that value, so the result
/// is exact regardless of vertex positions, coverage or rounding. The clear is red. This does not
/// show interpolation is correct: forwarding corner zero or averaging all three would pass.
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

/// A varying the corners disagree about produces a picture that is not uniform.
///
/// Two distant pixels differ. The exact value depends on barycentric weights, sample positions and
/// perspective correction, which are the driver's, so only "the varying varies" is claimed. With
/// the test above, that shows the attribute is interpolated rather than forwarded as a constant.
/// The gradient's direction is not checked.
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
        concat!(
            "opposite corners of the image are identical, so the varying was not interpolated ",
            "- a pipeline forwarding one corner's value to every fragment looks like this"
        )
    );
    // It is the varying showing, not the clear: the corners' alpha is one everywhere, and every
    // corner's colour has a channel the clear lacks.
    assert_eq!(first[3], 255, "the varying's alpha reached the attachment");
    assert_eq!(last[3], 255, "and at the far corner too");
}
