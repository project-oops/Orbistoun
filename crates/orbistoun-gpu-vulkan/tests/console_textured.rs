//! A textured pixel shader the hardware ran, on a device.
//!
//! Oracle record B is the GL cube with a texture on it. The first test drives its pixel shader from
//! the oracle's own triangle, isolating it; the second drives it from record B's own primitive
//! shader, fetching vertices from guest memory and exporting the coordinate the sample uses. The
//! record holds neither the vertex buffer nor the texture, so vertices and texels are this file's,
//! and a translated sample reads the one texture the pipeline bound (D690).

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

/// Where the textured pixel shader sits inside record B's payload image: the payload base plus
/// `0x200`, as `oracle_gl_cube.rs` pins it. Record A's untextured shader is at `0x300` of a
/// different payload.
const FRAGMENT_OFFSET: usize = 0x200;

/// Record B's payload image, read from the other crate's capture directory so the capture has one
/// copy.
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

/// A textured pixel shader the hardware ran samples a texture and puts it on the screen.
///
/// Every pixel is a texel of the bound texture, and the four quadrants are the four texels in the
/// places the coordinate names. The clear is a colour no texel carries. The texels and the vertex
/// program are this test's, so this is not the hardware's frame.
#[test]
fn the_consoles_textured_pixel_shader_samples_on_a_device() {
    if !device_or_skip("the_consoles_textured_pixel_shader_samples_on_a_device") {
        return;
    }
    let encodings = EncodingTable::builtin().expect("the shipped encoding table");
    let operands = OperandTable::builtin().expect("the shipped operand table");
    let payload = payload();
    // `decode_program`, not `decode`: a shader carries no length, so the terminator says where it
    // ends.
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

    // Two varyings, because the shader's declared inputs are locations zero and one; without the
    // second, every pixel samples the same unwritten coordinate. Location zero is the colour the
    // texel is modulated by, white. Location one is the coordinate, zero, two and two, sweeping the
    // texture once across the frame: the triangle's corners are at (-1, -1), (3, -1) and (-1, 3).
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

    // Every pixel is some texel: a band of clear between four correct samples would pass the checks
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

/// The low half of the guest address record B's vertex buffer sat at, `0x200900000`, the same as
/// record A's: both records are the same title drawing the same cube.
const VERTEX_BUFFER_BASE: u32 = 0x0090_0000;

/// Words in a window covering the vertex buffer and the canary the pixel shader writes.
const SPANNING_WORDS: u32 = 1 << 16;

/// Where each vertex sits in that window: twelve words apiece, four of position, four of colour,
/// four of texture coordinate, the layout record A's frame established.
const VERTEX_WORDS: [usize; 3] = [0, 12, 24];

/// Record B's own two shaders, drawing a textured frame together.
///
/// The primitive shader fetches vertices from guest memory and exports positions and texture
/// coordinates, and the textured pixel shader samples with what was interpolated. Both were
/// captured from one frame. The vertices and texels are this test's.
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
    // One span covering the vertex buffer and the canary, with both shaders translated against it,
    // so they agree about where guest memory starts.
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

    // A triangle covering the viewport, with texture coordinates sweeping the whole texture.
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
