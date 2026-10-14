# D862 - A render scale draws resident targets larger than the guest's

**Status:** measured
**Date:** 2026-10-14

**Amends:** D835 - a render resolution setting was rejected there until write-back could resample.

`config.toml`'s `[performance]` section takes a fourth setting, `render-scale`, and a title's own
`render_scale` replaces it for its runs, as the other three are (D835). It is how many times the
guest's size, across and down, a resident colour target and its depth target are drawn at: one to
four, one the guest's own.

A scaled target's guest-facing extent stays the guest's. Everything the guest sees passes through an
image of that extent, resampled on the device: its pixels are blitted down on the way to guest
memory - averaged where the format filters, the nearest texel for an integer or 32-bit float
target - and up, each guest pixel over its square, on the way in. The viewport, scissor and line
width a pipeline is built with are scaled with it. A depth target, which only a depth texture reads,
is drawn and sampled at the scaled extent.

The display keeps its own copy of a flipped frame at the drawn extent: `LazyCopies` takes a `show`
snapshot beside the one written to guest memory, and the window shows what it is given, scaled to
fit.

**Why:** a title drawing at its console size is soft on a larger display, and a player with the
host to spare can draw it sharper. Neverball's menu at 1920x1080 drawn at twice that ran 6.5 -> 4.2
frames a second in one run each, four times the pixels; the frame written back is the guest's
extent and matches one drawn at scale one.

**Known limits:**
- A pixel shader reading its own screen position sees the drawn one, not the guest's.
- A target sampled as a texture is read from guest memory, so a frame built from earlier passes
  (post-processing) sharpens only in its last pass.
- A flip shown from guest memory rather than from the device copy (a tiled scanout, D719) is shown
  at the guest's extent.

**Rejected:**
- Resampling on the host: a 2x 1920x1080 write-back is 33 MB to read and average a flip.
- Scaling the window's picture alone: it already does, and that only enlarges the guest's pixels.
