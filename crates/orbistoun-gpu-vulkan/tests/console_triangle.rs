//! The hardware's triangle, drawn from its captured command stream and compared with its own frame.
//!
//! The triangle record's vertex shader carries its three positions as constants, so walking the
//! stream, translating both shaders and driving the submission produces the hardware's own triangle
//! with no vertex buffer (D701). Every texel the hardware drew is reproduced pixel-exact. The field
//! the triangle does not cover differs: the backend clears to opaque black, while the hardware's
//! target held `0x55555555`, the surface's prior contents rather than anything in the draw.

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

/// The colour the triangle drew, and the colour the hardware's target was cleared to, as the bytes
/// a pixel comes back as: `0xff0000ff` and `0x55555555`, MSB-first.
const DRAWN: [u8; 4] = [0xff, 0x00, 0x00, 0xff];
const CONSOLE_CLEAR: [u8; 4] = [0x55, 0x55, 0x55, 0x55];
/// What the backend clears an attachment to: opaque black.
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

/// The two shader payloads, laid out at their guest addresses in one region.
///
/// A generous span, so the decoder's read, which narrows a 64 KiB window in powers of two, reaches
/// the 104-byte pixel shader's terminator. The bytes past each shader are zero and never read.
struct Shaders {
    bytes: Vec<u8>,
}

impl GuestMemory for Shaders {
    fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
        let offset = usize::try_from(address.checked_sub(VERTEX_ADDR)?).ok()?;
        self.bytes.get(offset..offset.checked_add(length)?)
    }
}

/// Reads one of the other crate's capture files as bytes, from there, so a capture has one copy.
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

/// A named hardware target, detiled to a linear frame of `[u8; 4]` pixels, MSB-first.
fn console_frame(target_capture: &str) -> Vec<[u8; 4]> {
    let target: Vec<u32> = capture(target_capture)
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    detile_64kb_rx_bpp4(&target, WIDTH, HEIGHT)
        .expect("the 64x64 target detiles")
        .into_iter()
        .map(|w| [(w >> 24) as u8, (w >> 16) as u8, (w >> 8) as u8, w as u8])
        .collect()
}

/// The hardware's triangle is reproduced pixel-exact; only the clear colour differs.
///
/// Walks the triangle DCB through `Pipeline::submit` with the two shaders served at their guest
/// addresses, drives the submission through a `VulkanBackend`, and compares `last_frame()` with the
/// detiled hardware target. The drawn pixels are checked at the hardware's own texels, so a shader
/// that coloured them differently or geometry that placed them elsewhere fails. The uncovered field
/// is the backend's black, not the target's prior clear, and that difference is asserted. Skips
/// with no device.
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

    // Both stages are bound with a module, and the draw is present, as `drive` needs.
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
    assert_eq!(
        outcome.refused, 0,
        "no command was refused: {:?}",
        outcome.refusals
    );
    let frame = backend.last_frame().expect("a frame was rendered");
    assert_eq!(
        (frame.width, frame.height),
        (WIDTH, HEIGHT),
        "the target sized the frame"
    );

    let console = console_frame("agc-primitive-draw-triangle-fw1240.target.hex");
    let mut drawn = 0usize;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let want = console[(y * WIDTH + x) as usize];
            let got = frame.at(x, y).expect("inside the frame");
            if want == DRAWN {
                // A texel the hardware drew: reproduced exactly.
                assert_eq!(
                    got, DRAWN,
                    "drawn texel ({x},{y}) is not the console's colour"
                );
                drawn += 1;
            } else {
                // A texel the hardware left at its clear: `0x55555555` there, the backend's black
                // here.
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

/// A point draw renders as the hardware's point, not the triangle the same bytes made.
///
/// The point record (`agc-primitive-draw-fw1240`) differs from the triangle record only in
/// `VGT_GS_OUT_PRIM_TYPE` (0 = POINTLIST against 2 = TRISTRIP) and two counts, and names the same
/// shader addresses, so the triangle's captured shaders drive it. The topology decodes to a point
/// list through the whole submit path without a device. With a device, the draw covers far fewer
/// texels than the triangle and lights the same texel as the hardware.
#[test]
fn a_point_draw_renders_as_a_point_not_a_triangle() {
    let stream = capture("agc-primitive-draw-fw1240.hex");
    let vertex = capture("agc-primitive-draw-triangle-fw1240.vertex.hex");
    let pixel = capture("agc-primitive-draw-triangle-fw1240.pixel.hex");
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

    // Device-free: the point record's topology is a point list through the whole submit path.
    assert_eq!(
        report.primitive_topology,
        Some(orbistoun_gpu::registers::PrimitiveTopology::PointList),
        "the point record's VGT_GS_OUT_PRIM_TYPE decodes to a point list"
    );

    if !device_or_skip("a_point_draw_renders_as_a_point_not_a_triangle") {
        return;
    }

    assert!(
        report.failures.is_empty(),
        "the point submission refused a shader: {:?}",
        report.failures
    );
    assert_eq!(
        report.shaders_translated, 2,
        "both shaders translate for a point draw"
    );

    let mut backend = VulkanBackend::new();
    let outcome = drive(&mut backend, &submission).expect("the point draw drives");
    assert_eq!(
        outcome.refused, 0,
        "no command was refused: {:?}",
        outcome.refusals
    );
    let frame = backend.last_frame().expect("a frame was rendered");
    assert_eq!(
        (frame.width, frame.height),
        (WIDTH, HEIGHT),
        "the target sized the frame"
    );

    // The hardware's point target, detiled like the triangle's.
    let console = console_frame("agc-primitive-draw-fw1240.target.hex");

    // Where the hardware lit a texel, and where orbistoun did.
    let console_drawn: Vec<(u32, u32)> = (0..HEIGHT)
        .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
        .filter(|&(x, y)| console[(y * WIDTH + x) as usize] == DRAWN)
        .collect();
    let orbistoun_drawn: Vec<(u32, u32)> = (0..HEIGHT)
        .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
        .filter(|&(x, y)| frame.at(x, y) == Some(DRAWN))
        .collect();
    println!("[point] console lit {console_drawn:?}, orbistoun lit {orbistoun_drawn:?}");

    // It is not the triangle: a point covers nowhere near the strip's 512 texels.
    assert!(
        orbistoun_drawn.len() < 512,
        "a point draw is not a triangle list: it drew {} texels, the triangle's whole count",
        orbistoun_drawn.len()
    );
    // The same texel as the hardware's point, pixel-exact.
    assert_eq!(
        orbistoun_drawn, console_drawn,
        "the point orbistoun drew is not the console's point"
    );
}
