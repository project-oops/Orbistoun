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
//! second external point. The first real multi-texel readback on offer is the triangle record (512
//! drawn pixels, `-f432`), not yet in the tree.
//!
//! # What waits on a backend (REQ-20260915T0929Z-36c0)
//!
//! The other half - orbistoun *produces* the frame by walking the stream through a backend, and the
//! produced frame must equal `agc-primitive-draw-fw1240.target.hex` - needs a backend on the run
//! path, which nothing has yet. When one lands, the produced frame is compared here against this same
//! recorded target. Until then the stream is checked to decode, and the target to detile.

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
