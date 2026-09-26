//! The framebuffer harness, checked against itself (D549).
//!
//! Clear an attachment to a colour and read that colour back. The only reference is the request,
//! which is what makes this harness checkable before anything is drawn through it.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::clear_to;

/// Reports the device once per test, or skips loudly.
///
/// A test that finds no device and reports success would make the suite green where nothing ran.
/// Repeated in each test binary rather than shared, since a shared helper is one more thing that
/// can be wrong.
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

/// A cleared attachment reads back as the colour it was cleared to.
///
/// Two clears, `(1, 0, 0, 1)` and `(0, 1, 0, 0)`, differ in every channel and so pin all four
/// against swaps. Every component is `0.0` or `1.0`, which `R8G8B8A8_UNORM` encodes exactly, so no
/// rounding is asserted. This covers image creation, the render pass, its layout transition, the
/// buffer copy and the readback, where a mistake yields plausible pixels rather than an error.
/// Nothing is drawn: the colour comes from `LOAD_OP_CLEAR`.
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

        // Every pixel, not a sample: a wrong copy extent or row length fills only part of the
        // buffer.
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
