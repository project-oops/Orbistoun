# 837. The guest's viewport transform is applied, and a GL frame is many submissions

**2026-09-24**: worklog 836 left Neverball drawing its backdrop and coins but no floor or walls.
This tick measured where every triangle goes, and found two things. One is fixed; the other is the
next unit.

## Measuring every submission, not the last

A GL frame is **many** submissions: the context submits whenever its 450-triangle vertex ring fills
(oops-sdk `gl_draw.c`). So `last-submission.txt` held only a frame's tail.

The worker now dumps the first 64 drawn submissions, in order, as `drawn-NN-*`:

- the command listing;
- the modules;
- the memory window;
- a quarter-scale snapshot of the frame each one left.

Scratch scripts (outside the tree) decoded every draw's clip-space positions at its real vertex size
(from its offset step), laid all the snapshots out as one contact sheet, and overlaid each
submission's triangles on its frame.

Neverball, 150 s, 15 submissions:

| submissions | what they draw |
|---|---|
| 0–3 | the sky sphere (16x128) and the first frame's geometry. Huge extents, many vertices behind the eye |
| 4–8 | the **coins** (256x256), every triangle |
| 9–14 | one dense 512x256-textured object, 360–420 triangles each, just past the top-right edge. Most likely the ball |

From submission 4 on, every snapshot shows the same static view. **At ~10 s per 450-triangle
submission, 150 s covers about two GL frames.** The run never reaches the rest of a frame: the menu
and logo GUI, and whatever else follows the ball. The missing floor is not yet explained. It could be
off-screen, absent from the title scene, or in a later submission. The speed is the wall.

## The fix: the guest's viewport transform

The coins' clip-space positions put them in the **upper** right (`y/w` from 0.4 to 1.7, GL `+y` up),
but orbistoun drew them in the **lower** right.

The GL context programs `PA_CL_VPORT_YSCALE = -height/2` (oops-sdk `gl_internal.h`, `gl_compute_vport`:
"NDC +y is up, rows grow down"). orbistoun ignored `PA_CL_VPORT_*` and used Vulkan's default mapping,
where NDC `+y` is down, so **every GL frame was drawn upside down**, the cube included.

- **`registers::ViewportTransform` and `viewport_transform_at`:** `PA_CL_VPORT_XSCALE/XOFFSET/YSCALE/YOFFSET`
  (`0xA10F`–`0xA112`, `gfx103.json`) as they stand at a draw. The result is `None` when any term is
  unwritten, or when `PA_CL_VTE_CNTL` (`0xA206`) is written without all four x/y enables. Both SDK
  paths write `0x43f`.
- **`RenderCommand::SetViewportTransform`:** emitted per draw when it changes, the same pattern as
  blend and user data.
- **`framebuffer::guest_viewport`:** `width = 2·XSCALE`, `x = XOFFSET − XSCALE`, and likewise for y. A
  GL guest's negative `YSCALE` becomes a negative viewport height, which Vulkan takes (core since 1.1)
  and which is exactly the flip.

The AGC draw path writes a **positive** `YSCALE` (`agc_draw.c:146-147`), the same as Vulkan's default.
So the console-triangle capture, which carries those registers, now goes through the transform and
still matches the console's target pixel for pixel. That is independent confirmation the mapping is
right.

Tests:
- `the_viewport_transform_is_read_with_its_sign` covers the decode, including before-the-writes and
  `VTE_CNTL`-disabled.
- `a_negative_y_scale_is_a_negative_height` covers the Vulkan viewport for both signs.

## What moved

- **The cube** is flipped vertically, with the same pixel histogram (63,293 colours, identical top
  four): the right orientation now, still inside-out without depth (`REQ-...2ea9`).
- **Neverball**, frame 14: planet top left, coins upper right, and the edge of the dense object at the
  top-right corner. This matches the clip-space coordinates.

## Next

**Speed.** Each draw copies the 1080p attachment in and out and builds its own pipeline. Rendering a
submission in one pass, with the attachment and pipelines kept resident across its draws, is what
lets a GL frame (and the GUI at its end) finish inside a run. After that come depth, and the floor
question.

## Gate state

`./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
