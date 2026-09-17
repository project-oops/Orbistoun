//! The console's **textured** pixel shader, on a device.
//!
//! Oracle record B is the GL cube with a texture on it. Its pixel shader was the last refusal in
//! either captured stream, and the image subsystem - a sampled image, a sampler, a binding, and
//! a translation for `image_sample_lz` - exists because of it (worklogs 566 and 568).
//!
//! It has translated since worklog 568 and had **never drawn**. That is the same gap the
//! levelled sample had until worklog 580: a translation nothing has run is a claim about a
//! module rather than about a frame.
//!
//! Two tests here, and the second is the one that matters. The first drives the pixel shader
//! from the oracle's own triangle, which isolates it: what it draws depends on nothing else that
//! could be wrong. The second drives it from **record B's own primitive shader**, fetching
//! vertices out of guest memory and exporting the coordinate the sample uses - the record's whole
//! frame path, with nothing hand-written in it.
//!
//! What neither can be is the console's frame. The record captured the command stream and the
//! shader payload and neither the vertex buffer nor the texture, so the vertices and texels here
//! are this file's - and which texture a translated sample reads is the one the pipeline bound,
//! for the reason D690 gives.

use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::draw_with_texture;
use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
use orbistoun_spirv::interpolating_vertex_module;
use orbistoun_translate::wavefront::{Stage, Window};
use orbistoun_translate::{Fidelity, Strategy, Width, translate_windowed};

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

/// Where the textured pixel shader sits inside record B's payload image.
///
/// The address the record's register writes name, as `oracle_gl_cube.rs` pins it: the payload
/// base plus `0x200`. Record A's untextured shader is at `0x300` of its own payload, and the two
/// are different shaders in different images - a detail worth stating because the offsets look
/// interchangeable and are not.
const FRAGMENT_OFFSET: usize = 0x200;

/// Record B's payload image, as bytes.
///
/// Read out of the other crate's capture directory rather than copied here, for the reason that
/// crate's own test gives: a second copy of a capture is a second thing to keep true.
fn payload() -> Vec<u8> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("orbistoun-gpu")
        .join("tests")
        .join("captures")
        .join("agc-gl-cube-fw1240-b.payload.hex");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut bytes = Vec::new();
    for line in text.lines() {
        for word in line
            .split('#')
            .next()
            .unwrap_or_default()
            .split_whitespace()
        {
            let value = u32::from_str_radix(word, 16)
                .unwrap_or_else(|e| panic!("{}: {word:?}: {e}", path.display()));
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

/// A two-by-two texture whose four texels are all different, first row then second.
///
/// `R8G8B8A8_UNORM` packs a word red first, so a word reads as `0xAABBGGRR`.
const TEXELS: [u32; 4] = [0xffff_0000, 0xff00_ff00, 0xff00_00ff, 0xff60_4020];

/// The same four, as the bytes a pixel comes back as.
const EXPECTED: [[u8; 4]; 4] = [
    [0, 0, 255, 255],
    [0, 255, 0, 255],
    [255, 0, 0, 255],
    [32, 64, 96, 255],
];

/// **A console's textured pixel shader samples a texture and puts it on the screen.**
///
/// # What it asserts
///
/// Every pixel of the frame is a texel of the bound texture, and the four quadrants are the four
/// texels, in the places the coordinate names. That is the same four-way claim the hand-written
/// oracle and the hand-assembled translation both make, over a shader **a console ran**.
///
/// The clear is a colour no texel carries, so a draw that produced nothing reads as a different
/// answer rather than an absent one.
///
/// # What it cannot assert
///
/// **That this is the console's frame.** The texels are this test's: the record captured the
/// command stream and the shader payload, not the texture. The vertex program is the oracle's
/// hand-written triangle rather than the console's own, so the coordinate sweeping the texture
/// is this file's arrangement too.
///
/// What it does establish is the thing the whole image subsystem was built for: the shader that
/// motivated it runs, samples, and produces a frame that depends on what it sampled.
#[test]
fn the_consoles_textured_pixel_shader_samples_on_a_device() {
    if !device_or_skip("the_consoles_textured_pixel_shader_samples_on_a_device") {
        return;
    }
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");
    let payload = payload();
    // `decode_program`, not `decode`: a shader carries no length and the payload image holds
    // more than this shader, so the terminator is what says where it ends.
    let decoded = decode_program(&payload[FRAGMENT_OFFSET..], &encodings, &operands);
    let strategy = Strategy::Predicated {
        fidelity: Fidelity::Auto,
        width: Width::default(),
    };
    let module = translate_windowed(
        &decoded,
        &encodings,
        strategy,
        Stage::Fragment,
        Window::default(),
    )
    .expect("the console's textured pixel shader translates")
    .module;
    println!("module: {} words", module.len());

    // **Two varyings, because the shader reads two.** Its declared inputs are locations zero and
    // one, which a scan of its own decorations says and a first attempt without the second
    // demonstrated: with only location zero supplied, the frame came back one texel everywhere -
    // the coordinate was whatever an unwritten input holds, which is the same for every pixel.
    //
    // Location zero is the colour it modulates the texel by, white so the texel passes through
    // unattenuated. Location one is the coordinate: zero, two and two, so it sweeps the whole
    // texture once across the visible frame - the triangle's corners are at (-1, -1), (3, -1)
    // and (-1, 3), two of them outside it.
    let colour = [[1.0f32, 1.0, 1.0, 1.0]; 3];
    let coordinate = [
        [0.0, 0.0, 0.0, 1.0],
        [2.0, 0.0, 0.0, 1.0],
        [0.0, 2.0, 0.0, 1.0],
    ];
    // Magenta, which is none of the four texels.
    let clear = [1.0, 0.0, 1.0, 1.0];
    let vertex = interpolating_vertex_module(&[colour, coordinate]);

    let pixels = draw_with_texture(&vertex, &module, clear, (8, 8), (&TEXELS, 2))
        .expect("the console's shader drew");

    let quadrants = [((1, 1), 0), ((6, 1), 1), ((1, 6), 2), ((6, 6), 3)];
    for ((x, y), texel) in quadrants {
        assert_eq!(
            pixels.at(x, y),
            Some(EXPECTED[texel]),
            concat!(
                "pixel ({}, {}) should be texel {}; magenta here means the console's shader ",
                "sampled nothing, and another texel means the coordinate reached the wrong one"
            ),
            x,
            y,
            texel
        );
    }

    // And every pixel is *some* texel, which is the coverage claim: a frame that sampled
    // correctly in four places and left a band of clear between them would pass the four checks
    // above.
    for y in 0..8 {
        for x in 0..8 {
            let pixel = pixels.at(x, y).expect("inside the frame");
            assert!(
                EXPECTED.contains(&pixel),
                "pixel ({x}, {y}) is {pixel:?}, which is no texel of the bound texture"
            );
        }
    }
}

/// The low half of the guest address record B's vertex buffer sat at, `0x200900000`.
///
/// The same address record A's does: the two records are the same title drawing the same cube,
/// and the capture that established this is shared between them (worklog 561).
const VERTEX_BUFFER_BASE: u32 = 0x0090_0000;

/// Words in a window covering the vertex buffer and the canary the pixel shader writes.
const SPANNING_WORDS: u32 = 1 << 16;

/// Where each vertex sits in that window, and how wide one is.
///
/// Twelve words apiece: four of position, four of colour, four of texture coordinate. That
/// layout is the one record A's frame established by drawing from it (worklog 565), and the
/// third group is the part this test needs and that one did not.
const VERTEX_WORDS: [usize; 3] = [0, 12, 24];

/// **Record B's own two shaders, drawing a textured frame together.**
///
/// # What this is
///
/// The console's primitive shader fetches vertices out of guest memory, exports their positions
/// and their texture coordinates, and the console's textured pixel shader samples with what the
/// hardware interpolated. Both shaders were captured from one frame; neither was written here.
///
/// Record A's pair does the same thing without a texture (worklog 565). This is the other
/// record's pair, and the difference between them is the whole image subsystem.
///
/// # What it cannot assert
///
/// **Not the console's frame.** The vertices and the texels are this test's - the record
/// captured the command stream and the shader payload, and neither the vertex buffer nor the
/// texture. The record's pixel hash remains a different claim needing both.
#[test]
fn record_bs_two_shaders_draw_a_textured_frame_together() {
    use orbistoun_gpu_vulkan::framebuffer::draw_mesh_over_texture;

    if !device_or_skip("record_bs_two_shaders_draw_a_textured_frame_together") {
        return;
    }
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");
    let payload = payload();
    let strategy = Strategy::Predicated {
        fidelity: Fidelity::Auto,
        width: Width::default(),
    };
    // One span covering the vertex buffer the primitive shader reads and the canary the pixel
    // shader writes, and **both shaders translated against it** - two shaders disagreeing about
    // where memory starts is a difference no picture would show (worklog 570).
    let window = Window::spanning(VERTEX_BUFFER_BASE, SPANNING_WORDS).expect("a power of two");

    let mesh = translate_windowed(
        &decode_program(&payload, &encodings, &operands),
        &encodings,
        strategy,
        Stage::Mesh,
        window,
    )
    .expect("the console's vertex program translates")
    .module;
    let fragment = translate_windowed(
        &decode_program(&payload[FRAGMENT_OFFSET..], &encodings, &operands),
        &encodings,
        strategy,
        Stage::Fragment,
        window,
    )
    .expect("the console's textured pixel shader translates")
    .module;

    // A triangle covering the viewport, with texture coordinates that sweep the whole texture
    // across it - the same zero, two and two the other tests use, for the same reason.
    let corners = [
        [-1.0f32, -1.0, 0.0, 1.0],
        [3.0, -1.0, 0.0, 1.0],
        [-1.0, 3.0, 0.0, 1.0],
    ];
    let coordinates = [
        [0.0f32, 0.0, 0.0, 1.0],
        [2.0, 0.0, 0.0, 1.0],
        [0.0, 2.0, 0.0, 1.0],
    ];
    let white = [1.0f32, 1.0, 1.0, 1.0];
    let mut memory = vec![0u32; SPANNING_WORDS as usize];
    for (vertex, at) in VERTEX_WORDS.into_iter().enumerate() {
        for component in 0..4 {
            memory[at + component] = corners[vertex][component].to_bits();
            memory[at + 4 + component] = white[component].to_bits();
            memory[at + 8 + component] = coordinates[vertex][component].to_bits();
        }
    }

    let clear = [1.0, 0.0, 1.0, 1.0];
    let drawn = draw_mesh_over_texture(&mesh, &fragment, clear, (8, 8), &memory, (&TEXELS, 2))
        .expect("the textured mesh draw ran");

    let quadrants = [((1, 1), 0), ((6, 1), 1), ((1, 6), 2), ((6, 6), 3)];
    for ((x, y), texel) in quadrants {
        assert_eq!(
            drawn.pixels.at(x, y),
            Some(EXPECTED[texel]),
            concat!(
                "pixel ({}, {}) should be texel {}; magenta here means the pair drew nothing, ",
                "and another texel means the coordinate the primitive shader exported did not ",
                "reach the sample"
            ),
            x,
            y,
            texel
        );
    }
    for y in 0..8 {
        for x in 0..8 {
            let pixel = drawn.pixels.at(x, y).expect("inside the frame");
            assert!(
                EXPECTED.contains(&pixel),
                "pixel ({x}, {y}) is {pixel:?}, which is no texel of the bound texture"
            );
        }
    }
}
