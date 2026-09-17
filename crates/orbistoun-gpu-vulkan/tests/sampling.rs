//! A texture read through a sampler on a device, which is where a guest's textured pixel
//! shader has to end up.
//!
//! Oracle record B is the GL cube with a texture on it, and its pixel shader ends in
//! `image_sample_lz` - the one family the translator still refuses. A guest's sampling
//! instruction takes its texture from a descriptor held in eight scalar registers and its
//! sampler from four more, and what those describe has to become an image, a view and a sampler
//! on the host. **None of that existed.**
//!
//! This is the host half standing up before anything is translated into it, which is the order
//! D549 argues for and the order both the fragment work and the mesh stage followed. The module
//! is hand-assembled and the texture is this file's; nothing here is translated, and nothing
//! here claims a guest's shader would do this.
//!
//! What it does establish is the thing the translation will need to be checked against: a known
//! texel, at a known coordinate, arriving in a known pixel.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::{TEXTURE_BINDING, draw_with_texture};
use orbistoun_spirv::{Lod, interpolated_vertex_module, sampling_fragment_module};

/// Whether a device is available at all, said aloud when it is not.
///
/// A machine with no device is a skip the caller has to see, never a pass - the same shape
/// every other device test here uses.
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

/// A two-by-two texture whose four texels are all different, in the order they are stored:
/// the first row left to right, then the second.
///
/// `R8G8B8A8_UNORM` packs a word red first, so a word reads as `0xAABBGGRR`. The fourth is a
/// colour nothing else here uses, so a pixel carrying it cannot have come from a clear, from a
/// default, or from one of the other three.
const TEXELS: [u32; 4] = [0xffff_0000, 0xff00_ff00, 0xff00_00ff, 0xff60_4020];

/// The same four, as the bytes a pixel comes back as.
const EXPECTED: [[u8; 4]; 4] = [
    [0, 0, 255, 255],
    [0, 255, 0, 255],
    [255, 0, 0, 255],
    [32, 64, 96, 255],
];

/// **A sampled pixel is the texel its coordinate names.**
///
/// # What it asserts
///
/// Four quadrants of one frame, each carrying a different texel of a two-by-two texture. That
/// is four claims in one draw and every one of them is needed:
///
/// - **The image is bound and read.** A pixel is a texel rather than the clear.
/// - **The coordinate reaches the sample.** Four coordinates give four different answers, so a
///   sampler ignoring its coordinate fails rather than passing by luck.
/// - **The rows are the right way up.** The texel a coordinate with the larger `v` names is the
///   one stored *later* in the buffer, which is the upload's claim about row order and the one
///   thing a staging copy can silently get backwards.
/// - **Nothing is filtered.** An exact byte match at a texel centre is only available because
///   the sampler does no filtering; a sampler that blended neighbours would fail here for a
///   reason about the sampler, which is worth being able to tell apart.
///
/// The coordinate is a varying, interpolated across the triangle, because that is what a
/// guest's textured shader has: its pixel shader reads a coordinate its vertex program exported
/// and the hardware interpolated. A constant would check less and look the same.
///
/// **Both sampling instructions, over the same claims.** A guest's `image_sample_lz` names level
/// zero explicitly; a fragment stage may instead let the implementation choose from the
/// derivatives of the coordinate. Those are different instructions with different operands, and
/// the one a translation will emit is the explicit one - so checking only the form that is
/// convenient here would leave the form that matters unrun.
///
/// # What it cannot assert
///
/// That a *translated* `image_sample_lz` would do this. Nothing is translated here. This is the
/// oracle that translation will be measured against, built first on purpose.
#[test]
fn a_sampled_pixel_is_the_texel_its_coordinate_names() {
    if !device_or_skip("a_sampled_pixel_is_the_texel_its_coordinate_names") {
        return;
    }

    // Eight by eight, so a texel boundary falls between pixels rather than through one. At
    // five rows the third row lands exactly on the boundary between the two texel rows, where
    // which side it falls on is the driver's fixed-point arithmetic rather than this test's
    // claim - so the frame is square and every sample sits well inside a texel.
    let size = (8, 8);

    // The visible frame is the clip-space square, and the triangle's corners are at (-1, -1),
    // (3, -1) and (-1, 3) - two of them outside it. A varying of 0, 2 and 2 therefore reaches
    // exactly 1 at the right and bottom edges of what is actually drawn, so the coordinate
    // sweeps the whole texture once across the frame.
    let corners = [
        [0.0, 0.0, 0.0, 1.0],
        [2.0, 0.0, 0.0, 1.0],
        [0.0, 2.0, 0.0, 1.0],
    ];
    // Magenta, which is none of the four texels: a pixel the draw never wrote reads as a
    // different answer rather than an absent one.
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
                // Spelled out rather than continued across lines: `cargo fmt` collapses a
                // line-continued literal and bakes the source indentation into what a failure
                // prints. The arguments are named because a format string built by a macro
                // cannot capture them from the surrounding scope.
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

        // And every pixel is *some* texel, which is the coverage claim: a frame where the
        // sampling worked in four places and left a band of clear between them would pass the
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

    // The two forms differ in which level they read, and this texture has one level - so they
    // have to agree byte for byte. A difference would mean one of them is reading something
    // other than the texture the other is.
    assert_eq!(
        frames[0].bytes, frames[1].bytes,
        "the implicit and level-zero samples disagree about a texture with one level"
    );
}

/// **The binding a module declares is the binding the harness fills.**
///
/// Two halves of one number live in two crates that cannot import each other: the module
/// builder says where a sampled image *is*, and this backend says where one is *bound*. A
/// backend that depended on the module builder would have the layering backwards - the builder
/// is a test dependency here deliberately - so the agreement is asserted rather than assumed.
///
/// It needs no device, so it runs everywhere, which is the point: a mismatch would otherwise
/// only show up as a validation error on a machine that has one.
#[test]
fn the_binding_a_module_declares_is_the_binding_the_harness_fills() {
    assert_eq!(
        TEXTURE_BINDING,
        orbistoun_spirv::TEXTURE_BINDING,
        "a module samples one binding and the harness fills another"
    );
}
