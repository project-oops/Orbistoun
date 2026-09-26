//! A texture read through a sampler on a device, where a guest's textured pixel shader is
//! translated to (D549).
//!
//! A guest's sampling instruction takes its texture from a descriptor in eight scalar registers and
//! its sampler from four more, which become an image, a view and a sampler on the host. The module
//! is hand-assembled and the texture is this file's: a known texel, at a known coordinate, arriving
//! in a known pixel, for a translated `image_sample_lz` to be checked against.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::{TEXTURE_BINDING, draw_with_texture};
use orbistoun_spirv::{Lod, interpolated_vertex_module, sampling_fragment_module};

/// Whether a device is available at all, said aloud when it is not: a missing device is a skip the
/// caller sees, never a pass.
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

/// A two-by-two texture whose four texels are all different, in storage order: the first row left
/// to right, then the second.
///
/// `R8G8B8A8_UNORM` packs a word red first, so a word reads as `0xAABBGGRR`. The fourth is a colour
/// nothing else here uses, so it cannot come from a clear or a default.
const TEXELS: [u32; 4] = [0xffff_0000, 0xff00_ff00, 0xff00_00ff, 0xff60_4020];

/// The same four, as the bytes a pixel comes back as.
const EXPECTED: [[u8; 4]; 4] = [
    [0, 0, 255, 255],
    [0, 255, 0, 255],
    [255, 0, 0, 255],
    [32, 64, 96, 255],
];

/// A sampled pixel is the texel its coordinate names.
///
/// Four quadrants of one frame, each a different texel of a two-by-two texture:
///
/// - The image is bound and read: a pixel is a texel rather than the clear.
/// - The coordinate reaches the sample: four coordinates give four answers.
/// - The rows are the right way up: the texel at larger `v` is the one stored later.
/// - Nothing is filtered: an exact byte match at a texel centre needs a non-filtering sampler.
///
/// The coordinate is an interpolated varying, as a guest's textured shader has. Both the
/// explicit-level form (as `image_sample_lz`) and the implicit-level form are checked. Nothing is
/// translated.
#[test]
fn a_sampled_pixel_is_the_texel_its_coordinate_names() {
    if !device_or_skip("a_sampled_pixel_is_the_texel_its_coordinate_names") {
        return;
    }

    // Eight by eight, so every sample sits well inside a texel: at five rows the third row lands on
    // a texel boundary, where the driver's fixed-point arithmetic decides the side.
    let size = (8, 8);

    // The triangle's corners are at (-1, -1), (3, -1) and (-1, 3), so a varying of 0, 2 and 2
    // reaches exactly 1 at the right and bottom edges of the visible square, sweeping the texture
    // once.
    let corners = [
        [0.0, 0.0, 0.0, 1.0],
        [2.0, 0.0, 0.0, 1.0],
        [0.0, 2.0, 0.0, 1.0],
    ];
    // Magenta, none of the four texels, so an unwritten pixel is a different answer.
    let clear = [1.0, 0.0, 1.0, 1.0];

    let mut frames = Vec::new();
    for lod in [Lod::Implicit, Lod::Zero] {
        let pixels = draw_with_texture(
            &interpolated_vertex_module(corners),
            &sampling_fragment_module(lod),
            clear,
            size,
            (&TEXELS, 2),
        )
        .expect("the sampling draw ran");

        // One pixel well inside each quadrant, and which texel each one has to be.
        let quadrants = [((1, 1), 0), ((6, 1), 1), ((1, 6), 2), ((6, 6), 3)];
        for ((x, y), texel) in quadrants {
            assert_eq!(
                pixels.at(x, y),
                Some(EXPECTED[texel]),
                // Spelled out rather than line-continued, because `cargo fmt` collapses a continued
                // literal and bakes the indentation into the message. Named arguments, because a
                // macro-built format string cannot capture from scope.
                concat!(
                    "{:?}: pixel ({}, {}) should be texel {}; magenta here means the draw ",
                    "wrote nothing, and another texel means the coordinate reached the wrong one"
                ),
                lod,
                x,
                y,
                texel
            );
        }

        // Every pixel is some texel: a band of clear between four correct samples would pass the
        // checks above.
        for y in 0..8 {
            for x in 0..8 {
                let pixel = pixels.at(x, y).expect("inside the frame");
                assert!(
                    EXPECTED.contains(&pixel),
                    concat!(
                        "{:?}: pixel ({}, {}) is {:?}, which is no texel of the bound ",
                        "texture"
                    ),
                    lod,
                    x,
                    y,
                    pixel
                );
            }
        }
        frames.push(pixels);
    }

    // The texture has one level, so the two forms must agree byte for byte.
    assert_eq!(
        frames[0].bytes, frames[1].bytes,
        "the implicit and level-zero samples disagree about a texture with one level"
    );
}

/// The binding a module declares is the binding the harness fills.
///
/// The module builder says where a sampled image is, and this backend says where one is bound; the
/// backend cannot depend on the builder, so the agreement is asserted. It needs no device, so a
/// mismatch shows everywhere rather than as a validation error on a machine with one.
#[test]
fn the_binding_a_module_declares_is_the_binding_the_harness_fills() {
    assert_eq!(
        TEXTURE_BINDING,
        orbistoun_spirv::TEXTURE_BINDING,
        "a module samples one binding and the harness fills another"
    );
}
