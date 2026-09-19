# 701. The console's triangle, rendered through the backend and compared with its own frame

**2026-09-19** — inbox `-f50b`: walk the triangle DCB (`-f432`) through `Pipeline::submit`, drive the
submission through a `VulkanBackend`, and compare `last_frame()` with the detiled console target.
`crates/orbistoun-gpu-vulkan/tests/console_triangle.rs`. This is **the first render orbistoun has
checked against a frame the console drew** - every framebuffer test before it compares orbistoun
against material orbistoun generated (D701).

## The result: the triangle is pixel-exact

Both console shaders translate (the vertex, 44 dwords at `0x2000c0000`; the pixel, 26 at
`0x2000c0200`), the draw runs, and the frame comes back 64×64. **Every one of the 512 texels the
console drew is reproduced exactly** - the triangle's shape and its `0xff0000ff` colour, from the
console's own GCN shaders, needing no vertex buffer because the vertex program carries its three
positions as constants (obSCEne `-b9d2`). The register vocabulary that names the shader addresses is
the least certain table in the GPU crate; a frame that lands its pixels where the console's did is the
strongest evidence yet that what it found were shaders and where they drew.

## The one gap it names: the clear colour

The frame is **not** equal to the console's everywhere, and the single reason is the clear. On the
field the triangle did not cover, the backend's attachment is opaque black `[0,0,0,255]`; the
console's target read back `0x55555555`. That clear is not in the draw the stream describes - it is
the surface's prior contents - so `drive` clears to its own default and cannot match it. **The next
backend gap for a full-frame match is the target carrying its initial clear colour into the render**
(whether the DCB sets a clear register the decode misses, or the clear was a fill outside the captured
stream, is the question that gap opens with). Asserted, not papered over: the test checks the drawn
texels against the console and the undrawn ones as the black-vs-`0x55555555` difference, so the gap is
on record and a change to it fails the test.

## One test-harness note

The pixel shader first refused - "no end-of-program within 64 bytes of `0x2000c0200`". Not the
shader: the decoder narrows its read window in powers of two, and a `GuestMemory` holding exactly the
104-byte shader lands it on 64 bytes, short of the terminator. A padded region (the bytes past a
shader are zero, which the decoder never reaches - it stops at the shader's own end first) reads it
whole and it translates. A note for the next captured shader served to the pipeline in a test.

## Watched failing

Per principle 3: asserting the backend clear equals the console's `0x55555555` fails at once ("the
clear gap changed"), so the clear difference is a measurement, not an assumption; and the drawn texels
are checked at the console's own positions, so a shader that recoloured them or geometry that moved
them would fail the comparison.

## Gate state

`./bin/orbistoun check` passes end-to-end (all checks passed); the device test skips cleanly where
there is no device, like the crate's others (a device was present here). `orbistoun-gpu-vulkan` clippy
`-D warnings` clean, fmt clean, prose exit 0, `status --check` exit 0, doc gate clean, identity scan
clean. Corpus unchanged. No commit.
