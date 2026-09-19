//! The console's triangle, drawn from its captured command stream and compared with its own frame.
//!
//! # The first render checked against a frame the console drew
//!
//! Every framebuffer check before this compares orbistoun against material orbistoun generated
//! (D701). `submitted_frame.rs` walks a captured stream and draws its translated shaders, but over
//! *this file's* geometry - the gl-cube stream carries no vertex buffer. The triangle record does not
//! need one: per obSCEne `-b9d2`'s disassembly the vertex shader carries its three positions as
//! constants, so walking the stream, translating both shaders, and driving the submission produces
//! the console's own triangle - the same 512 texels, at the same places, in the same colour.
//!
//! # The outcome, and the one gap it names (`-f50b`)
//!
//! Both shaders translate and the draw runs. Every one of the console's 512 drawn texels is
//! reproduced **pixel-exact**. The frame is *not* equal to the console's target everywhere, and the
//! single reason is the clear: the backend clears the attachment to opaque black, while the console's
//! target read back `0x55555555` on the field the triangle did not cover. That clear is not in the
//! draw the stream describes - it is the surface's prior contents - so a full-frame match waits on the
//! target carrying its clear colour into the render, which is the next backend gap this records.
//!
//! So this asserts what does hold, exactly: the drawn triangle, pixel for pixel against the detiled
//! console target; and the clear difference, named rather than papered over.

use orbistoun_gpu::pipeline::{GuestMemory, Pipeline, Queue};
use orbistoun_gpu::{RenderCommand, ShaderStage, detile_64kb_rx_bpp4, drive};
use orbistoun_gpu_vulkan::VulkanBackend;
use orbistoun_gpu_vulkan::compute::{Availability, probe};
use orbistoun_translate::{Fidelity, Strategy, Width};

/// The guest addresses the stream's registers name the two shaders at.
const VERTEX_ADDR: u64 = 0x2_000c_0000;
const PIXEL_ADDR: u64 = 0x2_000c_0200;
const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;

/// The colour the triangle drew, and the colour the console's target was cleared to, as the bytes a
/// pixel comes back as. `0xff0000ff` and `0x55555555`, MSB-first.
const DRAWN: [u8; 4] = [0xff, 0x00, 0x00, 0xff];
const CONSOLE_CLEAR: [u8; 4] = [0x55, 0x55, 0x55, 0x55];
/// What the backend clears an attachment to today: opaque black.
const BACKEND_CLEAR: [u8; 4] = [0x00, 0x00, 0x00, 0xff];

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

/// The two shader payloads, laid out at their console addresses in one region.
///
/// A generous span so the decoder's read is not starved: it narrows a 64 KiB window in powers of two,
/// and a buffer exactly a shader's length lands it on 64 bytes, short of the 104-byte pixel shader's
/// terminator. The bytes past each shader are zero, which the decoder never reaches - it stops at the
/// shader's own end-of-program first.
struct Shaders {
    bytes: Vec<u8>,
}

impl GuestMemory for Shaders {
    fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
        let offset = usize::try_from(address.checked_sub(VERTEX_ADDR)?).ok()?;
        self.bytes.get(offset..offset.checked_add(length)?)
    }
}

/// Reads one of the other crate's capture files as bytes - from there, not copied, so a capture stays
/// one thing to keep true.
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

/// The console's target, detiled to a linear frame of `[u8; 4]` pixels, MSB-first.
fn console_frame() -> Vec<[u8; 4]> {
    let target: Vec<u32> = capture("agc-primitive-draw-triangle-fw1240.target.hex")
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    detile_64kb_rx_bpp4(&target, WIDTH, HEIGHT)
        .expect("the 64x64 target detiles")
        .into_iter()
        .map(|w| [(w >> 24) as u8, (w >> 16) as u8, (w >> 8) as u8, w as u8])
        .collect()
}

/// **The console's triangle is reproduced pixel-exact; only the clear colour differs.**
///
/// Walks the triangle DCB through `Pipeline::submit` with the two shaders served at their console
/// addresses, drives the resulting submission through a `VulkanBackend`, and compares `last_frame()`
/// against the detiled console target. Both shaders translate, the draw runs, and every one of the
/// 512 texels the console drew comes back exactly - the triangle's shape and colour reproduced from
/// the console's own shaders. The field the triangle did not cover is the backend's black rather than
/// the console's `0x55555555`, which is the target's prior clear and not in the draw; that difference
/// is asserted, not hidden, and is the next backend gap for a full-frame match. Skips with no device.
///
/// Discriminating, not vacuous: the drawn pixels are checked at the console's own texels, so a shader
/// that coloured them differently, or geometry that placed them elsewhere, fails the comparison.
#[test]
fn the_console_triangle_is_reproduced_pixel_exact_over_a_black_clear() {
    if !device_or_skip("the_console_triangle_is_reproduced_pixel_exact_over_a_black_clear") {
        return;
    }

    let stream = capture("agc-primitive-draw-triangle-fw1240.hex");
    let vertex = capture("agc-primitive-draw-triangle-fw1240.vertex.hex");
    let pixel = capture("agc-primitive-draw-triangle-fw1240.pixel.hex");
    assert_eq!(
        PIXEL_ADDR - VERTEX_ADDR,
        0x200,
        "the shaders sit 0x200 apart"
    );
    let mut bytes = vec![0u8; 0x1000];
    bytes[..vertex.len()].copy_from_slice(&vertex);
    bytes[0x200..0x200 + pixel.len()].copy_from_slice(&pixel);
    let memory = Shaders { bytes };

    let mut pipeline = Pipeline::new(Strategy::Predicated {
        fidelity: Fidelity::Auto,
        width: Width::default(),
    })
    .expect("a pipeline over the built-in tables");

    let submission = pipeline.submit(&stream, Queue::Draw, &[], &memory);
    let report = &submission.report;
    assert!(
        report.failures.is_empty(),
        "the submission refused a shader: {:?}",
        report.failures
    );
    assert_eq!(
        report.shaders_found, 2,
        "the registers name a vertex and a pixel shader"
    );
    assert_eq!(
        report.shaders_translated, 2,
        "both console shaders translate"
    );
    assert_eq!(report.draws, 1, "the stream describes one draw");

    // Both stages are bound with a module, and the draw is present - what `drive` needs.
    let bound: Vec<&ShaderStage> = submission
        .commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::BindShader { stage, shader } => {
                assert!(
                    submission.modules.contains_key(shader),
                    "{stage:?} has no module"
                );
                Some(stage)
            }
            _ => None,
        })
        .collect();
    assert!(
        bound.contains(&&ShaderStage::Vertex) && bound.contains(&&ShaderStage::Fragment),
        "both a vertex and a fragment shader are bound: {bound:?}"
    );

    let mut backend = VulkanBackend::new();
    let outcome = drive(&mut backend, &submission).expect("the console triangle drives");
    assert_eq!(outcome.refused, 0, "no command was refused");
    let frame = backend.last_frame().expect("a frame was rendered");
    assert_eq!(
        (frame.width, frame.height),
        (WIDTH, HEIGHT),
        "the target sized the frame"
    );

    let console = console_frame();
    let mut drawn = 0usize;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let want = console[(y * WIDTH + x) as usize];
            let got = frame.at(x, y).expect("inside the frame");
            if want == DRAWN {
                // A texel the console drew: orbistoun reproduces it exactly.
                assert_eq!(
                    got, DRAWN,
                    "drawn texel ({x},{y}) is not the console's colour"
                );
                drawn += 1;
            } else {
                // A texel the console left at its clear: the console's `0x55555555`, the backend's
                // black. This is the named gap - the target's prior clear the draw does not carry.
                assert_eq!(
                    want, CONSOLE_CLEAR,
                    "an undrawn console texel is not the clear colour"
                );
                assert_eq!(
                    got, BACKEND_CLEAR,
                    "the backend clear at ({x},{y}) is not opaque black - the clear gap changed"
                );
            }
        }
    }
    assert_eq!(drawn, 512, "the console drew 512 texels and all reproduced");
}
