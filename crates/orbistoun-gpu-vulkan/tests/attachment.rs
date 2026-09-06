//! The framebuffer harness, checked against itself.
//!
//! Clear an attachment to a colour and read that colour back. There is nothing to compare
//! against but the request, and that is the point: every other part of phase 6 is verified
//! against material this project generated, so it cannot be wrong in a way its own tests would
//! notice. This one can (D549).

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::clear_to;

/// Reports the device once per test, or skips **loudly**.
///
/// A test that finds no device, returns early and reports success would make the suite green on
/// a machine where the only thing it checks never ran. The compute side follows the same rule
/// and this repeats it rather than sharing it, because a helper shared between two test binaries
/// is a third thing that can be wrong.
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

/// **A cleared attachment reads back as the colour it was cleared to.**
///
/// # What this asserts
///
/// Two clears, not one, and the pair is what makes it a check rather than a demonstration.
/// `(1, 0, 0, 1)` and `(0, 1, 0, 0)` differ in every channel, so between them they pin all four:
/// one colour alone would leave a red/alpha swap invisible, since both are `255` in the first,
/// and a green/blue swap invisible in the second.
///
/// Every component is `0.0` or `1.0`, which `R8G8B8A8_UNORM` encodes exactly as `0` and `255`.
/// No rounding is being asserted, so a failure here is a real disagreement rather than a
/// half-bit.
///
/// It exercises the whole attachment path: image creation, the render pass, the layout
/// transition it performs on the way out, the buffer copy, and the readback. A mistake in any
/// of those produces *plausible* pixels rather than an error, which is why this is built and
/// checked before the draw that will sit on top of it.
///
/// # What it cannot assert
///
/// **That anything was drawn.** Nothing is - there is no pipeline and no shader here, and the
/// colour comes from `LOAD_OP_CLEAR`. A working clear says the plumbing carries pixels from an
/// attachment to the host; it says nothing about whether a fragment shader's output reaches the
/// same place, which is the next step.
///
/// It also asserts nothing about *hardware* fidelity. Like the compute harness, this verifies
/// this project's own code against its own request on whatever device is present.
#[test]
fn a_cleared_attachment_reads_back_as_the_colour_it_was_given() {
    if !device_or_skip("a_cleared_attachment_reads_back_as_the_colour_it_was_given") {
        return;
    }

    for (colour, expected) in [
        ([1.0, 0.0, 0.0, 1.0], [255u8, 0, 0, 255]),
        ([0.0, 1.0, 0.0, 0.0], [0u8, 255, 0, 0]),
    ] {
        let pixels = clear_to(colour, 4, 3).expect("the attachment path ran");
        assert_eq!(pixels.width, 4);
        assert_eq!(pixels.height, 3);
        assert_eq!(
            pixels.bytes.len(),
            4 * 3 * 4,
            "the readback is tightly packed, so it is four bytes a pixel and nothing more"
        );

        // Every pixel, not a sample. A copy region with the wrong extent or row length fills
        // part of the buffer correctly and leaves the rest, which one probed pixel would miss.
        for y in 0..pixels.height {
            for x in 0..pixels.width {
                assert_eq!(
                    pixels.at(x, y),
                    Some(expected),
                    "pixel ({x}, {y}) after clearing to {colour:?}"
                );
            }
        }
        assert_eq!(
            pixels.at(4, 0),
            None,
            "reading outside the image answers nothing"
        );
    }
}
