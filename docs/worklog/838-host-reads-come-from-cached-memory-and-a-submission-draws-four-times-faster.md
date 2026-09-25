# 838. Host reads come from cached memory, and a submission draws four times faster

**2026-09-24**: worklog 837 found that one Neverball GL frame spans many submissions at about 10 s
each, so a 150 s run saw about two frames and never reached their end. This tick measured where a
draw's ~25 ms went and removed most of it.

## The measurement

Temporary timing around `framebuffer::render_over` and the backend's draw call, removed after use:

| stage | before |
|---|---|
| setup: attachment, render pass, framebuffer, readback buffer | ~0.7–1.7 ms |
| pipeline build, including the translated modules' compile | ~1–3.5 ms |
| GPU work and wait | ~0.6–0.9 ms |
| **after the GPU**: map, read 8 MB of pixels, window readback, release | **~21 ms** |
| the backend storing its result twice | ~1.2 ms |

The pipeline build is not the cost; reading the frame back is.

## The cause

`create_host_buffer` took the **first** memory type that is `HOST_VISIBLE | HOST_COHERENT`. On this
device (an NVIDIA RTX 5070 Ti) that is write-combined: fast for the host to write, and read at a
small fraction of cached speed. Every draw read its 1080p result, 8 MB, back out of it.

## The changes

- **`framebuffer::create_host_buffer` prefers `HOST_CACHED`:** a type that is also cached, falling
  back to the old choice where a device has none. Coherent either way, so no flushing changes. After
  it, the post-GPU segment is ~2.5 ms.
- **`VulkanBackend` keeps each draw's result once:** `last_pixels`, a second 8 MB copy of what
  `contents` already held, becomes `last_drawn`, the target whose `contents` entry is the last frame.

## What moved

Neverball submissions take **2.5–2.8 s, from 9.6–11.6 s**. A 150 s run draws **28 submissions**
(from 14).

The later frames show scene elements never reached before:

- a **yellow cylinder with sparkles** at the lower left, most likely the goal;
- in the last frame, a **black rotated square** drawn over it. It is not yet identified: GUI, shadow
  or something else. That is the next question.

The cube now confirms **15 frames** in its run (from 5). Its later frames draw the GL demo's **HUD
overlay**: a translucent panel with legible text, upright, reading "GL1 CUBE … Cull ON Light ON Tex
OFF Depth ON". That is text through oops-gl's font path, blended, confirming worklog 837's
orientation fix. The cube itself is still inside-out, and the HUD names the reason: culling and
depth, `REQ-...2ea9`.

Of the 31 Neverball submissions, 29 ran to completion and 28 of those with draws. The last stopped at
its draw, consistent with the run's time limit arriving mid-frame. It was not investigated further
this tick.

## Next

- The remaining per-draw cost is the 8 MB round trip itself. Keeping the attachment resident on the
  device across a submission's draws, reading it back once at the end, is the next multiple.
- After that: depth and cull (`REQ-...2ea9`), and what the black square is.

## Gate state

`./bin/orbistoun check` **fails on one step this tick did not touch**:

- `status --check` reports "README.md has no generated block - the markers are missing".
- Another session edited `README.md` at 13:51 on 2026-09-24. It removed the generated "Where It
  Actually Is" block (and the clean-room section) in favour of a pointer to `docs/PROJECT_STATUS.md`,
  and the status gate still expects the block's markers.
- Left as that session left it. Whether the gate or the README changes is not this loop's call.

Every other step passes, including clippy. Its first run failed on this tick's own
`Option<Option<ResourceId>>`, which is now a named `DrawnOn`. Worklog index regenerated, identity scan
clean. No commit.
