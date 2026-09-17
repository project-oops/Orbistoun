# 638. A draw renders into the guest's target size, not the interim square

**2026-09-16** - the render-target dimensions decoded last time (worklog 637) now reach the pixels: a
colour target is a residency resource, `SetRenderTargets` selects it, and a `Draw` sizes its
attachment from it instead of the fixed 64x64 square

## What this joins

Worklog 637 decoded `CB_COLOR0_ATTRIB2` into a `ColourTargetExtent` and stopped there, a measured
primitive nothing consumed. This threads it through to the frame:

- **The colour target is a resource** (`Resource::RenderTarget { width, height }`), the enum growing
  the arm D701 said it would. It carries no bytes - a backend draws into an attachment it allocates
  and reads back, so what it needs is the size, not the guest's pixels - which is what lets
  `orbistoun-gpu` hand it over without naming a graphics API.
- **The frontend emits it.** `submit` decodes the extent (`colour_target_extent_at`), carries the
  target in `Submission.targets`, and emits a `SetRenderTargets` selecting it, before the draws -
  target state a draw reads.
- **The driver makes it resident**, in the same pass as the shader modules (`render::drive`).
- **The backend sizes the draw from it.** `VulkanBackend` records a resident target's dimensions,
  `SetRenderTargets` selects one, and `draw_graphics` renders into `(width, height)` from the
  selection - or the interim square when none is set.

## Verified on the device

`a_draw_renders_into_the_selected_target_size`: a 128x96 colour target is made resident and selected,
a fullscreen triangle is drawn, and the frame comes back **128x96** - a size the fixed `RENDER_WIDTH`
square could not produce - with its centre the fragment's blue. That the dimensions reach the
attachment *and* the pipeline still draws is the join between the register decode and the pixels. The
non-device half is pinned too: a `SetRenderTargets` naming a target that was never made resident is
refused with `UnknownResource` (not drawn to the wrong size), and an empty set clears the selection
back to the interim square.

## Made to fail

- `a_target_size_write_reaches_the_backend_as_set_render_targets` (frontend) - a stream writing
  `CB_COLOR0_ATTRIB2 = 0x000fc03f` (obSCEne's measured 64x64 value) produces one target of 64x64 and
  a `SetRenderTargets` selecting it; before, a stream's target size reached the backend nowhere.
- `a_render_target_is_made_resident` (driver) - the target is made resident alongside the modules
  (`resident: 2`), against a driver that made only modules resident.
- `a_draw_renders_into_the_selected_target_size` / `an_unresident_render_target_is_refused` /
  `an_empty_render_target_set_clears_the_selection` (backend), above.

## Two things settled in passing

- **A second oracle for the size decode.** obSCEne's constructed draw stream writes
  `CB_COLOR0_ATTRIB2 = 0x000fc03f` for 64x64, which decodes to exactly 64x64 - a second measured
  source agreeing with the hardware capture's 1920x1080, from a different tool.
- **The base register is disputed, so the id is the extent.** The capture and obSCEne's stream name
  *different* offsets for `CB_COLOR0_BASE` (0x318 vs 0x200) while agreeing on the size register, so a
  render target is identified by its extent, not its address, in a top-bit-tagged id namespace
  disjoint from the sequential shader ids. Recorded as **D702**, including when that has to change (a
  per-target host object makes the address load-bearing).

## A test the change dated, and one loosened on purpose

- `an_unimplemented_command_is_refused_by_name` executed a `SetRenderTargets` and asserted it was
  refused - true until this unit taught it to select a target. Repointed at `SetViewport`, which
  genuinely has no execution yet, so the guard still checks that an unimplemented command names
  itself.
- `an_arbitrary_command_stream_is_survived` asserted `commands.len() <= shaders_translated`. That was
  always a loose proxy - a `Draw` or `Dispatch` breaks it too, and it passed only because the seed
  produced none - and a `SetRenderTargets` from a random target-size write would trip it. Tightened
  to its documented intent (bound shaders <= translated), which is what the comment always claimed and
  is robust to render-state commands.

## What is still interim, named as such

- **No clear colour.** Neither capture writes a clear-colour register and none has a settled offset,
  so the attachment stays interim black rather than inventing one (D702, principle 3).
- **One target per frame, colour only.** The latest `CB_COLOR0_ATTRIB2` is decoded once; per-draw
  target changes and depth targets are later arms, and the frontend emits `depth: None`.
- **The draw still draws three vertices.** Vertex-input decode (a guest's vertex buffers and counts)
  is the next graphics piece; indexed draws, viewport and clears still refuse by name.

## Gate state

`cargo test --workspace` **2400 passed, 0 failed** (+5: two frontend/driver, three backend);
`cargo clippy --workspace --all-targets -D warnings` clean; fmt clean; identity scan exit 0. D702
written and indexed. No commit.
