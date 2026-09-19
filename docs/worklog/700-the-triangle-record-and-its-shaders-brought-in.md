# 700. The triangle record, its target and both shaders, brought into the tree

**2026-09-19** — inbox `-f432`: the tree had the point record (sweep `20260916-223136`, one drawn
pixel) but not the triangle (sweep `20260917-124503`, 512 drawn pixels), and the shader bytecode
worklog 686 filed for arrived since (obSCEne `-5ecd`/`-b9d2`). A single point cannot tell one correct
swizzle from another; a triangle has a shape. This brings the triangle's whole record in, from one
sweep, and detiles it to that shape.

## Extracted byte-exact from the sweep report

obSCEne's `166-agc/primitive-draw` in sweep `20260917-124503` recorded, as `OBS|bytes|...` lines, the
DCB stream, the 64×64 colour target, and both shader payloads. Transcribed mechanically the way 686
did the point record - each 4-byte group reversed so `read_words`'s `to_le_bytes` reproduces the raw
bytes - and verified against the report's own byte counts:

- `agc-primitive-draw-triangle-fw1240.hex` - the DCB, **1892 bytes** (473 dwords)
- `.target.hex` - the colour target, **16384 bytes** (4096 words): **exactly 512** `0xff0000ff`, 3584
  `0x55555555`, and nothing else
- `.vertex.hex` - the vertex shader, **44 dwords** at guest `0x2000c0000`
- `.pixel.hex` - the pixel shader, **26 dwords** at `0x2000c0200`

Each file carries a header naming the sweep. The extraction was byte-count-checked against the report
(`dcb-stream` 1892, `color-target` 16384, `vs-bytecode` 176, `ps-bytecode` 104), not eyeballed. Only
the exact `166-agc/primitive-draw` check was taken - the report also carries `-clip`, `-param3`,
`-fixture` and `ngg-` variants of the same streams, and none of those bytes are in these files.

## The tests, and what the triangle gives that the point could not

`tests/primitive_draw.rs`:

- **`the_recorded_triangle_draw_stream_decodes`** - the console's triangle DCB walks whole, every
  opcode known, the walk consuming the full stream. The strip submission carries no packet the point's
  did not; the decoder already knew all of it.
- **`detiling_the_recorded_triangle_reconstructs_the_filled_triangle`** - the payoff. Detiled, the 512
  texels form **one filled triangle**, asserted as a shape rather than a pixel list: exactly 512 drawn
  and the rest clear; every drawn row a gap-free run; the runs narrowing top-to-bottom and
  mirror-symmetric; all inside the measured footprint `x[16..=47] y[16..=46]` (32 px wide at the top,
  2 at the point). This is the first *multi-texel* external framebuffer signal in the tree - a real
  shape the swizzle is checked against, not a lone point on a field that cannot discriminate.
- **`the_triangle_record_carries_both_shader_payloads`** - pins the two payloads' dword counts so a
  mis-extraction is caught now, not when `-f50b`'s render-and-compare first binds them.

**Watched failing** (principle 3): substituting the point record's one-pixel target for the triangle's
fails `detiling_...` at once, at "the console drew 512 pixels" (it finds 1). Reverted, it greens.

## Gate state

`./bin/orbistoun check` passes end-to-end (all checks passed): `orbistoun-gpu` tests 6 pass (3 point,
3 triangle), clippy `-D warnings` clean, fmt clean, prose exit 0, `status --check` exit 0, doc gate
clean, identity scan clean. `vocabulary.rs` skips these files (no `.toml`, as the point record has
none - they are console dumps, not captured library calls). Corpus unchanged. No commit.

The render-and-compare that consumes the shaders is `-f50b`; the recorded target and shaders it needs
are now in the tree.
