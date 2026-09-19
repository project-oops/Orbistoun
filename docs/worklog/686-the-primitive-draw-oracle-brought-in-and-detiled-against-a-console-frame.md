# 686. The primitive-draw oracle is in the tree, and tiling.rs now detiles a whole console frame

**2026-09-17** — inbox `-445f`: the primitive-draw hardware record that `tiling.rs` founds its swizzle
equation on was cited by the tree and not in it - one anchor byte was used and the other 4,095 pixels
left in obSCEne's report. This brings the record in and turns that single anchor into a full-frame
check. Also recorded the resolution of obSCEne `-7c21`.

## The record, extracted from the report it was measured in

obSCEne's `166-agc/primitive-draw` (retail FW 12.40, sweep `20260916-223136`) submitted a ~1.9 KB DCB
that draws one point and read the whole 64x64 32-bpp colour target back. Both are in the report as
`dcb-stream|offset|hex` and `color-target|offset|hex` byte lines, so the transcription is mechanical,
not hand-typed: each 4-byte group reversed (so `read_words`'s `to_le_bytes` reproduces the raw bytes),
verified byte-exact - stream **1892 bytes** (`0x764`), target **16384 bytes** (64x64x4), the point
`0xff0000ff` at byte **4348**, the clear colour `0x55555555` everywhere else. Landed as
`crates/orbistoun-gpu/tests/captures/agc-primitive-draw-fw1240.hex` and `.target.hex`.

## Two checks that need no rendering backend

`tests/primitive_draw.rs`:

- **`the_recorded_point_draw_stream_decodes`** - a *console-produced* DCB (not a constructed stream
  like `graphics_draw.rs`) walks whole, every opcode known. The decode-only assertion 445f asks for.
- **`detiling_the_recorded_target_reconstructs_the_point_on_a_cleared_field`** - the payoff:
  `detile_64kb_rx_bpp4` applied to all 4,096 recorded texels reconstructs the linear frame, checked
  point-and-field. This is the **first external framebuffer signal in the repository that runs** -
  every prior one is orbistoun against material orbistoun generated (D701), and the gl-cube captures'
  console hashes only print because their buffers are unmapped. That the 16 KB dump detiled at all
  proves the surface is tightly packed in one block, not a sparse 64 KB tile.
- **`the_swizzle_anchor_holds_the_point`** - the direct one-byte check `tiling.rs` was fitted to.

Watched failing before trusted (445f, principle 3): pointing the expected texel at `(16,15)` fails
both the anchor and the full-frame test; reverting greens them.

## What waits, and the request filed for it

The render-and-compare half - orbistoun *produces* the frame and it must equal the recorded target -
waits on a backend being on the run path (`-36c0`). It also needs the **shader bytecode**, which is
the one piece of the capture the record's own dump lacks: `166-agc/create-shader` captured the shader
object header and the DCB carries the shader addresses, but not the payload instructions. Filed
obSCEne `-b9d2` for the two shader payloads so the comparison is ready when a backend lands.

## 7c21 recorded

obSCEne `-7c21` came back: the unnamed libSceAgc NID `0x7d86501b8094ef57` at the leading titles' wall
is **not** a retail export (FW 12.40, three sweeps) and no vendor name exists (834,780 words searched).
Updated `docs/HANDOVER-OBSCENE.md`'s note on it from "any new name the census reports, orbistoun can
place" to the settled verdict: no name will arrive, dispatch by NID if a handler is ever wanted. The
other NID there (`0x53bbd82b51d172db`) remains open.

## Gate state

`orbistoun-gpu` `primitive_draw` 3 pass, `vocabulary` unaffected (lone `.hex` ignored by its `.toml`
discovery); `clippy -p orbistoun-gpu --all-targets -D warnings` clean; `./bin/orbistoun prose` exit 0;
`cargo fmt --check` clean; identity scan clean. No commit.
