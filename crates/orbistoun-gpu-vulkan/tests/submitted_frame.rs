//! A frame drawn from a **captured command stream**, rather than from shaders picked out by hand.
//!
//! # What has and has not happened before this
//!
//! Both halves existed and had never met. `orbistoun-gpu`'s oracle test walks a captured stream,
//! finds the shader addresses its register writes name, reads those shaders out of guest memory
//! and translates them - and stops there, because that crate deliberately knows no graphics API.
//! This crate draws translated modules, and every test that does so far reached into the payload
//! image at a hardcoded offset for them.
//!
//! So nothing had ever gone from *a guest asked for this* to a frame. That is the submission
//! pipeline's whole purpose, and it is the last join in the GPU path that nothing had exercised.
//!
//! # What this is not
//!
//! **Not the console's frame.** The stream names its shaders and its draw; it does not carry the
//! vertex buffer or the texture, and the record's pixel hash needs both. The vertices and texels
//! here are this file's, exactly as in `console_textured.rs`.
//!
//! What it does establish is that the addresses the register vocabulary extracts are the
//! addresses of shaders that run.

use orbistoun_gpu::pipeline::{GuestMemory, Pipeline, Queue};
use orbistoun_gpu::{RenderCommand, ShaderStage};
use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_gpu_vulkan::framebuffer::draw_mesh_over_texture;
use orbistoun_translate::wavefront::Window;
use orbistoun_translate::{Fidelity, Strategy, Width};

fn device_or_skip(what: &str) -> bool {
    match probe() {
        Availability::Available { properties } if properties.mesh_shading => {
            println!("[{what}] device: {}", properties.device);
            true
        }
        Availability::Available { properties } => {
            println!(
                concat!(
                    "[{}] SKIPPED - {} has no mesh stage, so a guest's geometry cannot run here"
                ),
                what, properties.device
            );
            false
        }
        Availability::Unavailable { reason } => {
            println!("[{what}] SKIPPED - no device: {reason}");
            false
        }
    }
}

/// Where the shader payload sat on the console; the address the stream's registers name.
const PAYLOAD_ADDRESS: u64 = 0x2_008f_0000;

/// The low half of the guest address the vertex buffer sat at.
const VERTEX_BUFFER_BASE: u32 = 0x0090_0000;

/// Words in a window covering the vertex buffer and the canary the pixel shader writes.
const SPANNING_WORDS: u32 = 1 << 16;

/// Where each vertex sits in that window: twelve words, of position, colour and coordinate.
const VERTEX_WORDS: [usize; 3] = [0, 12, 24];

/// A two-by-two texture whose four texels are all different, first row then second.
const TEXELS: [u32; 4] = [0xffff_0000, 0xff00_ff00, 0xff00_00ff, 0xff60_4020];

/// The same four, as the bytes a pixel comes back as.
const EXPECTED: [[u8; 4]; 4] = [
    [0, 0, 255, 255],
    [0, 255, 0, 255],
    [255, 0, 0, 255],
    [32, 64, 96, 255],
];

/// The payload image, as the guest's address space sees it.
struct Payload {
    bytes: Vec<u8>,
}

impl GuestMemory for Payload {
    fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
        let offset = usize::try_from(address.checked_sub(PAYLOAD_ADDRESS)?).ok()?;
        self.bytes.get(offset..offset.checked_add(length)?)
    }
}

/// Reads one of the other crate's capture files as bytes.
///
/// Read from there rather than copied here, for the reason that crate's own test gives: a second
/// copy of a capture is a second thing to keep true.
fn capture(name: &str) -> Vec<u8> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("orbistoun-gpu")
        .join("tests")
        .join("captures")
        .join(name);
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

/// **A captured command stream produces a frame.**
///
/// # What it asserts
///
/// The submission is walked, its register writes name two shaders, both translate, and the two
/// modules it hands back draw the four-quadrant texture frame every other test in this crate
/// asserts. Nothing between the stream and the pixels is chosen here: **which** module is the
/// geometry and which is the shading comes from the stage the pipeline attributed, not from an
/// offset this file knows.
///
/// That last part is the claim. The register vocabulary that extracts a shader address is the
/// least certain table in the GPU crate, by its own comment. A frame drawn from what it found is
/// the first evidence that what it found were shaders rather than plausible numbers.
///
/// # What it cannot assert
///
/// That the frame is the console's, for the reasons this file opens with. And it says nothing
/// about the *draw* the stream describes - the vertex count, the topology, the render target -
/// because this draws through the harness rather than through anything that reads those.
#[test]
fn a_captured_command_stream_draws_a_frame() {
    if !device_or_skip("a_captured_command_stream_draws_a_frame") {
        return;
    }
    let stream = capture("agc-gl-cube-fw1240-b.hex");
    let memory = Payload {
        bytes: capture("agc-gl-cube-fw1240-b.payload.hex"),
    };

    // One span covering the vertex buffer and the canary, given to the pipeline rather than to
    // each translation - which is what `with_window` exists for, and what a submission could not
    // say until worklog 571.
    let window = Window::spanning(VERTEX_BUFFER_BASE, SPANNING_WORDS).expect("a power of two");
    let mut pipeline = Pipeline::new(Strategy::Predicated {
        fidelity: Fidelity::Auto,
        width: Width::default(),
    })
    .expect("a pipeline over the built-in tables")
    .with_window(window);

    let submission = pipeline.submit(&stream, Queue::Draw, &[], &memory);
    println!(
        "packets {}, shaders found {}, translated {}, failures {:?}",
        submission.report.packets,
        submission.report.shaders_found,
        submission.report.shaders_translated,
        submission.report.failures,
    );
    assert!(
        submission.report.failures.is_empty(),
        "the submission refused a shader: {:?}",
        submission.report.failures
    );

    // Which module is which comes from the stage the pipeline attributed. A guest's vertex
    // program becomes a mesh module here (D688), so the vertex stage is the geometry.
    let mut geometry = None;
    let mut shading = None;
    for command in &submission.commands {
        if let RenderCommand::BindShader { stage, shader } = command {
            let module = submission.modules.get(shader);
            match stage {
                ShaderStage::Vertex => geometry = module,
                ShaderStage::Fragment => shading = module,
                ShaderStage::Compute => {}
            }
        }
    }
    let geometry = geometry.expect("the stream named a vertex program and it translated");
    let shading = shading.expect("the stream named a pixel shader and it translated");

    // A triangle covering the viewport, with coordinates sweeping the texture across it.
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
    let mut vertices = vec![0u32; SPANNING_WORDS as usize];
    for (vertex, at) in VERTEX_WORDS.into_iter().enumerate() {
        for component in 0..4 {
            vertices[at + component] = corners[vertex][component].to_bits();
            vertices[at + 4 + component] = white[component].to_bits();
            vertices[at + 8 + component] = coordinates[vertex][component].to_bits();
        }
    }

    // Magenta, which is none of the four texels.
    let clear = [1.0, 0.0, 1.0, 1.0];
    let drawn = draw_mesh_over_texture(geometry, shading, clear, (8, 8), &vertices, (&TEXELS, 2))
        .expect("the submitted frame drew");

    let quadrants = [((1, 1), 0), ((6, 1), 1), ((1, 6), 2), ((6, 6), 3)];
    for ((x, y), texel) in quadrants {
        assert_eq!(
            drawn.pixels.at(x, y),
            Some(EXPECTED[texel]),
            concat!(
                "pixel ({}, {}) should be texel {}; magenta here means the modules the stream ",
                "named drew nothing, and another texel means the coordinate went astray"
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
