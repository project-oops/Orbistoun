# 823. The render log lists a submission's commands — and a live submission's shaders have no window onto their own vertices, so every triangle collapses

**2026-09-24** — worklog 822 made a frame's draws accumulate, and a full census still found not one
pixel covered by either baseline's draws. The render log said how many commands ran and which were
refused; it did not say what they were.

## The command summary

`render::render_submission_to` now prints the submission's commands before driving them — each run of
identical consecutive commands once, with its count, at most twelve lines (`command_summary`). Bounded,
so it is always on. The cube's:

```
SetRenderTargets { colour: [ResourceId(...)], depth: None }
SetViewport(Rect { x: 0, y: 0, width: 1920, height: 1080 })
BindShader { stage: Vertex, shader: ResourceId(1) }
BindShader { stage: Fragment, shader: ResourceId(2) }
Draw { vertices: 3, instances: 1, first_vertex: 0 } x12
```

Everything a cube needs: one 1920x1080 target, a full-screen viewport, both shaders bound, twelve
triangles. So the state is right and the geometry is what is missing.

## Why the geometry is missing

The GL context's vertex stage is an NGG primitive shader that **fetches its vertices from guest
memory** — it forms a 64-bit vertex-buffer address from user data with a shift and a carry-in add
(`tools/shader-fixtures/primitive.s`). A translated shader reaches guest memory through one storage
buffer, the pipeline's `Window` (`orbistoun-translate` `wavefront.rs`): a power-of-two span of words at
a base, every access checked against it, anything outside refused.

- **The submit path never places the window.** `agc_driver::submit_described` builds its pipeline with
  `Pipeline::new(..)` and no `with_window`, so the window is the default — 64 words at address zero —
  which `read_guest_window` documents as "unmapped … reads nothing". Every vertex fetch the cube's
  primitive shader makes is refused, and every triangle's three vertices collapse to the same point.
- **It could not place one if it tried.** `Window.base` is a `u32`, and the cube's buffers live in
  direct memory around `0x7400_01xx_xxxx`, far above four gigabytes.
- **Why the oracle tests draw anyway**: `console_fragment`, `console_textured` and `submitted_frame`
  relocate the captured vertex buffer to `0x0090_0000` and hand the pipeline a window spanning it by
  hand (worklog 571). A live submission has neither the relocation nor the window.

So the gap is structural rather than a missing instruction: a live guest's shaders cannot see the memory
their own submission names. Closing it needs the window's base widened to 64 bits and the window
**derived from the submission** — the vertex-buffer address the stream's user data carries — which is a
decision (a mechanism the pipeline does not have), taken with its record next.

## Gate state

`crates/orbistoun-worker/src/render.rs` (`command_summary`, `SUMMARY_LINES`, the print). A diagnostic;
the guests run as in worklog 822. `./bin/orbistoun check` green, worklog index regenerated, identity
scan clean. No commit.
