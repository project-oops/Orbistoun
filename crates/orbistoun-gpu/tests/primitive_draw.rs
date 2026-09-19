//! The point-draw oracle, brought into the tree.
//!
//! obSCEne's `166-agc/primitive-draw` (retail FW 12.40, sweep `20260916-223136`) submitted a ~1.9 KB
//! DCB that draws one point and read the 64x64 32-bpp colour target back. `tiling.rs` founds its
//! swizzle on **one byte** of that record - texel `(15,15)` -> byte 4348 - and this brings in the
//! rest: the full command stream and the whole target readback
//! (`captures/agc-primitive-draw-fw1240.hex` and `.target.hex`).
//!
//! # What the target readback is, and is not
//!
//! It is **one drawn pixel on a uniform clear field**: `0xff0000ff` at texel `(15,15)`, and
//! `0x55555555` in the other 4,095 words. So detiling it checks *where that one texel lands* against
//! the one measured byte, and that the swizzle is a bijection over the surface - **not** 4,096
//! independent measurements: a uniform field cannot tell one correct swizzle from another that keeps
//! the point unique. It re-confirms the single anchor through the full-target path; it does not add a
//! second external point. The first real multi-texel readback is the triangle record - 512 drawn
//! pixels in a filled triangle - now in the tree beside it (`agc-primitive-draw-triangle-fw1240.*`,
//! sweep `20260917-124503`, `-f432`), and detiled to that shape below.
//!
//! # The other half, and where it now can run (REQ-20260915T0929Z-36c0)
//!
//! The other half - orbistoun *produces* the frame by walking the stream through a backend, and the
//! produced frame is compared against the recorded target - needs no *run path*: it is a test.
//! `console_triangle.rs` (`orbistoun-gpu-vulkan`, `-f50b`) drives the triangle record's `Submission`
//! through `VulkanBackend` and reproduces the console's 512 drawn texels pixel-exact (only the clear
//! colour differs). A backend on the run path is what a live *run* still lacks (36c0); that comparison
//! needs none. Here the stream is checked to decode and the target to detile.

use orbistoun_gpu::{detile_64kb_rx_bpp4, tiled_byte_offset_64kb_rx_bpp4, walk};
use std::path::PathBuf;

mod common;

/// The point the draw places, and the value the target is cleared to - both measured
/// (`color-val 0xff0000ff`, and every other target word read back `0x55555555`).
const POINT: u32 = 0xff00_00ff;
const CLEAR: u32 = 0x5555_5555;
/// The target is a 64x64 32-bpp surface; the point's texel is fixed by the capture's geometry
/// (NDC `(-0.5,-0.5)` through a scale/offset-32 viewport to screen `(16,16)`, texel `(15,15)`).
const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;
const POINT_X: u32 = 15;
const POINT_Y: u32 = 15;

fn captures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("captures")
}

fn target_words() -> Vec<u32> {
    let bytes = common::read_words(&captures_dir().join("agc-primitive-draw-fw1240.target.hex"));
    bytes
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// **The recorded ~1.9 KB point-draw stream decodes whole, with no unknown packet.**
///
/// The decode-only half of the oracle: a console-produced DCB (not a constructed one, as
/// `graphics_draw.rs` uses) walks exactly, so every opcode a real draw submission carries is known.
#[test]
fn the_recorded_point_draw_stream_decodes() {
    let bytes = common::read_words(&captures_dir().join("agc-primitive-draw-fw1240.hex"));
    let walk = walk(&bytes);
    assert!(
        walk.is_trustworthy(),
        "desynchronised={} overran={} trailing={} - the console's own draw stream did not decode",
        walk.desynchronised,
        walk.overran,
        walk.trailing_bytes,
    );
    let consumed: u32 = walk.packets.iter().map(|p| p.length).sum();
    assert_eq!(
        consumed as usize,
        bytes.len(),
        "the walk consumes the whole 0x764-byte stream"
    );
}

/// **The anchor `tiling.rs` is built on is exactly where the point is in the recorded target.**
///
/// A direct check of the one byte the swizzle equation was fitted to: texel `(15,15)` maps to byte
/// 4348, and that word of the recorded target is the point's colour.
#[test]
fn the_swizzle_anchor_holds_the_point() {
    let target = target_words();
    let offset = tiled_byte_offset_64kb_rx_bpp4(POINT_X, POINT_Y);
    assert_eq!(offset, 4348, "the measured anchor byte");
    assert_eq!(
        target[offset as usize / 4],
        POINT,
        "the point's colour is at the swizzled offset"
    );
}

/// **Detiling the recorded target puts the drawn pixel back at its texel on a cleared field.**
///
/// Apply the swizzle to all 4,096 texels and confirm the linear image is the clear colour everywhere
/// except the point's texel. What this actually discriminates is narrow, because the field is uniform:
/// a swizzle that sent the drawn pixel to the wrong byte, or another texel to the pixel's byte, fails;
/// a swizzle wrong only among the identical clear words does not. So this pins the one measured point
/// through the full-target path, not 4,096 independent pixels.
#[test]
fn detiling_the_recorded_target_reconstructs_the_point_on_a_cleared_field() {
    let target = target_words();
    let linear = detile_64kb_rx_bpp4(&target, WIDTH, HEIGHT).expect("the 64x64 target detiles");
    assert_eq!(linear.len(), (WIDTH * HEIGHT) as usize);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let expected = if (x, y) == (POINT_X, POINT_Y) {
                POINT
            } else {
                CLEAR
            };
            assert_eq!(
                linear[(y * WIDTH + x) as usize],
                expected,
                "texel ({x},{y}) of the detiled console frame"
            );
        }
    }
}

/// The triangle-strip record's colour target, as words - the multi-texel companion to the point.
fn triangle_target_words() -> Vec<u32> {
    let bytes =
        common::read_words(&captures_dir().join("agc-primitive-draw-triangle-fw1240.target.hex"));
    bytes
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// **The recorded triangle-strip stream decodes whole, with no unknown packet.**
///
/// The same decode-only check as the point stream, on the console's triangle DCB (sweep
/// `20260917-124503`): every opcode a real strip submission carries is known, and the walk consumes
/// the whole stream.
#[test]
fn the_recorded_triangle_draw_stream_decodes() {
    let bytes = common::read_words(&captures_dir().join("agc-primitive-draw-triangle-fw1240.hex"));
    let walk = walk(&bytes);
    assert!(
        walk.is_trustworthy(),
        "desynchronised={} overran={} trailing={} - the console's triangle stream did not decode",
        walk.desynchronised,
        walk.overran,
        walk.trailing_bytes,
    );
    let consumed: u32 = walk.packets.iter().map(|p| p.length).sum();
    assert_eq!(
        consumed as usize,
        bytes.len(),
        "the walk consumes the whole triangle stream"
    );
}

/// **Detiling the triangle target reconstructs a filled triangle of exactly 512 drawn pixels.**
///
/// The payoff the single point could not give - a uniform field pins only *where one texel lands*.
/// Here 512 texels of `0xff0000ff` sit on the clear field, and detiled they form one filled
/// triangle: a real shape to check the swizzle against. Asserted as the shape, not a pixel list -
/// exactly 512 drawn and the rest clear (so the point record's one-pixel target substituted here
/// fails at once), every drawn row a gap-free run, the runs narrowing top to bottom and
/// mirror-symmetric, all inside the measured footprint `x[16..=47] y[16..=46]`.
#[test]
fn detiling_the_recorded_triangle_reconstructs_the_filled_triangle() {
    // The triangle's screen footprint, measured off the detiled target.
    const MIN_X: u32 = 16;
    const MAX_X: u32 = 47;
    const MIN_Y: u32 = 16;
    const MAX_Y: u32 = 46;

    let target = triangle_target_words();
    let linear = detile_64kb_rx_bpp4(&target, WIDTH, HEIGHT).expect("the 64x64 target detiles");
    assert_eq!(linear.len(), (WIDTH * HEIGHT) as usize);
    let texel = |x: u32, y: u32| linear[(y * WIDTH + x) as usize];

    // Exactly 512 drawn, everything else the clear colour and nothing a third value.
    let drawn = linear.iter().filter(|&&w| w == POINT).count();
    let clear = linear.iter().filter(|&&w| w == CLEAR).count();
    assert_eq!(drawn, 512, "the console drew 512 pixels");
    assert_eq!(
        drawn + clear,
        linear.len(),
        "every texel is the point colour or the clear colour"
    );

    // The drawn pixels form a filled triangle: each row a single gap-free run, inside the footprint,
    // mirror-symmetric, and never wider than the row above (narrowing to a point at the bottom).
    let mut previous_width = WIDTH + 1;
    for y in 0..HEIGHT {
        let xs: Vec<u32> = (0..WIDTH).filter(|&x| texel(x, y) == POINT).collect();
        if xs.is_empty() {
            assert!(
                !(MIN_Y..=MAX_Y).contains(&y),
                "row {y} is inside the footprint yet drew nothing"
            );
            continue;
        }
        let (lo, hi) = (xs[0], *xs.last().expect("non-empty"));
        assert!(
            (MIN_Y..=MAX_Y).contains(&y),
            "a drawn row {y} outside the footprint"
        );
        assert!(
            lo >= MIN_X && hi <= MAX_X,
            "row {y} runs outside the footprint"
        );
        assert_eq!(
            xs.len() as u32,
            hi - lo + 1,
            "row {y} of the triangle has a hole"
        );
        assert_eq!(lo - MIN_X, MAX_X - hi, "row {y} is not mirror-symmetric");
        let width = hi - lo + 1;
        assert!(
            width <= previous_width,
            "row {y} is wider than the one above it"
        );
        previous_width = width;
    }
}

/// **The triangle record carries both shader payloads, at the sizes the DCB names them.**
///
/// Brought in for the render-and-compare half (`-f50b`) to bind at the addresses the stream carries -
/// vertex at guest `0x2000c0000`, pixel at `0x2000c0200`. Their dword counts (44 and 26) are pinned
/// here so a mis-extraction is caught now, not when f50b first reads them.
#[test]
fn the_triangle_record_carries_both_shader_payloads() {
    let vs =
        common::read_words(&captures_dir().join("agc-primitive-draw-triangle-fw1240.vertex.hex"));
    let ps =
        common::read_words(&captures_dir().join("agc-primitive-draw-triangle-fw1240.pixel.hex"));
    assert_eq!(vs.len(), 44 * 4, "the vertex shader is 44 dwords");
    assert_eq!(ps.len(), 26 * 4, "the pixel shader is 26 dwords");
}

/// **The recorded draws carry the primitive topology their `VGT_GS_OUT_PRIM_TYPE` sets.**
///
/// `-0c58`: a draw's topology is read, not assumed. The point record's register is POINTLIST (0) and
/// the triangle record's is TRISTRIP (2), so walking each yields a distinct topology - the signal that
/// tells a point draw from a triangle one, which the input-assembly register (`TRILIST` for both)
/// cannot. Stated by register value; and the decode names its four values and carries anything else by
/// its raw field, so an unmapped topology cannot pass as a triangle (watched here).
#[test]
fn the_recorded_draws_carry_their_primitive_topology() {
    use orbistoun_gpu::PrimitiveTopology;
    use orbistoun_gpu::registers::{
        Vocabulary, decode_primitive_topology, primitive_topology_at, register_writes,
    };

    let vocab = Vocabulary::builtin().expect("vocab");
    let topology = |name: &str| {
        let bytes = common::read_words(&captures_dir().join(name));
        primitive_topology_at(&register_writes(&walk(&bytes), &bytes, &vocab))
    };

    // The point record sets VGT_GS_OUT_PRIM_TYPE = 0 (POINTLIST); the triangle record = 2 (TRISTRIP).
    assert_eq!(
        topology("agc-primitive-draw-fw1240.hex"),
        Some(PrimitiveTopology::PointList),
        "the point record's draw is a point list"
    );
    assert_eq!(
        topology("agc-primitive-draw-triangle-fw1240.hex"),
        Some(PrimitiveTopology::TriangleStrip),
        "the triangle record's draw is a triangle strip"
    );

    // The decode names the register's four values and carries the rest by its raw field, so an
    // unmapped topology is refused rather than read as a triangle.
    assert_eq!(decode_primitive_topology(0), PrimitiveTopology::PointList);
    assert_eq!(decode_primitive_topology(1), PrimitiveTopology::LineStrip);
    assert_eq!(
        decode_primitive_topology(2),
        PrimitiveTopology::TriangleStrip
    );
    assert_eq!(
        decode_primitive_topology(3),
        PrimitiveTopology::RectangleList
    );
    assert_eq!(decode_primitive_topology(7), PrimitiveTopology::Other(7));

    // And each names itself for a report - the unmapped one by its raw field, not a fabricated name.
    assert_eq!(PrimitiveTopology::PointList.label(), "point list");
    assert_eq!(PrimitiveTopology::TriangleStrip.label(), "triangle strip");
    assert_eq!(PrimitiveTopology::Other(7).label(), "VGT_GS_OUTPRIM_TYPE 7");
}

/// **A submission carries the topology it decoded, on its report.**
///
/// `-0c58`, acceptance (1) end-to-end: walking the triangle record through `Pipeline::submit` yields a
/// `Submission` whose report names the draw a triangle strip - the topology is carried, not assumed.
/// (The mesh output still emits triangles regardless; making it follow this is the rest of `-0c58`.)
#[test]
fn a_submission_carries_its_primitive_topology() {
    use orbistoun_gpu::PrimitiveTopology;
    use orbistoun_gpu::pipeline::{GuestMemory, Pipeline, Queue};
    use orbistoun_translate::{Fidelity, Strategy, Width};

    struct NoMemory;
    impl GuestMemory for NoMemory {
        fn read(&self, _address: u64, _length: usize) -> Option<&[u8]> {
            None
        }
    }

    let stream = common::read_words(&captures_dir().join("agc-primitive-draw-triangle-fw1240.hex"));
    let mut pipeline = Pipeline::new(Strategy::Predicated {
        fidelity: Fidelity::Auto,
        width: Width::default(),
    })
    .expect("pipeline");
    let submission = pipeline.submit(&stream, Queue::Draw, &[], &NoMemory);
    assert_eq!(
        submission.report.primitive_topology,
        Some(PrimitiveTopology::TriangleStrip),
        "the submission report names the triangle record a triangle strip"
    );
}
