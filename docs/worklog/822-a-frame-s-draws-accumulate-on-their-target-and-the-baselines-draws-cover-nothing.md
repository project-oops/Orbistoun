# 822. A frame's draws accumulate on their target instead of each starting from black — and a full census shows the baselines' draws cover no pixel at all

**2026-09-24** — worklog 821 found both baselines' first frames uniformly black and named two causes:
the backend's black clear where the guest's target held its own contents (`REQ-...77fa`), and draws
that leave nothing visible. Reading the backend for the first turned up something that makes the
second impossible to see.

## Every draw began from a cleared attachment

`orbistoun-gpu-vulkan`'s draw path builds a fresh attachment per `Draw`: its render pass loads with
`LOAD_OP_CLEAR` to opaque black, runs the one draw, and copies the image out; `last_frame` is that one
draw's result. **A frame's draws never met.** Neverball's 450-draw title screen read back as whatever
the 450th draw covered, on black; the cube's 12 faces, as its last face.

## Draws now start from what their target holds

- **`framebuffer`**: a draw can start from given pixels. `Bound` carries `initial` (tightly packed
  `Rgba8`, used only when it covers the attachment exactly). When present, the readback buffer carries
  the pixels in before it carries the result out: `copy_in` moves the attachment `UNDEFINED ->
  TRANSFER_DST_OPTIMAL` and copies the buffer into it, the render pass takes that as its initial layout
  and **loads** instead of clearing, and a second external dependency orders the copy before the pass
  touches the attachment. The attachment gains `TRANSFER_DST` usage and the buffer `TRANSFER_SRC`.
  `draw_vertices` and `draw_mesh_into` take the start as `(clear, initial)`.
- **`VulkanBackend`** keeps `contents` — each target's pixels after the draws on it so far, keyed by the
  target `SetRenderTargets` selected — and starts each draw from them when the size matches, storing the
  result back. The first draw on a target, or one whose size changed, starts from the clear as before.
- **Test `a_frames_draws_accumulate_on_their_target`** (on the device): green into the left half, then
  red into the right half of one target. The left staying green is what the old behaviour fails. All
  seventeen `orbistoun-gpu-vulkan` test binaries pass.

## What the baselines' frames are now

```
cube:      16 commands, 0 refused, 1920x1080 - non-black pixels: 0
Neverball: 454 commands, 0 refused, 1920x1080 - non-black pixels: 0   (every pixel counted)
```

Still black, and now the census says more than it could before: with every draw landing on the same
accumulating target, **not one draw of either guest covers a single pixel**. That is not the clear and
not accumulation; it is the geometry — what the translated primitive shaders emit as positions and
primitives, the viewport and scissor a draw is restricted to, or the vertex fetch from the guest-memory
window — and it is the next unit, with the cube (12 simple faces) as the place to find it.

`REQ-...77fa` is **not** closed. This is the half it needs underneath it — a target that can start from
given pixels — but seeding those pixels from the guest's own surface (`submission.colour_target`,
detiled), and the full-frame console-triangle comparison its acceptance names, are still to do. The
guest surfaces of both baselines are 1920x1080 64KB_R_X, beyond the one 64 KiB block
`detile_64kb_rx_bpp4` models, so their seeding also needs the multi-block layout.

## Gate state

`crates/orbistoun-gpu-vulkan/src/framebuffer.rs` (`initial`, `copy_in`, the loading render pass, usages,
the two entry points), `crates/orbistoun-gpu-vulkan/src/lib.rs` (`contents`, the draw arm, the test).
`./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
