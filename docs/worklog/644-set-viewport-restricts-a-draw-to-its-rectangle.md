# 644. SetViewport restricts a draw to its rectangle - the backend half of the viewport

**2026-09-16** - a `SetViewport` is no longer refused: the executor applies it as the scissor, so a
draw paints only inside the rectangle and pixels outside keep the clear. The frontend decode of the
viewport register is deferred - no capture in hand sets one

## What is buildable, and what is not

The viewport splits the way the render target and the texture did: what the guest *declared* and what
a host *does* with it are separate, and only one is blocked.

- **The register decode is blocked on a capture.** None of the captures in hand sets a scissor or a
  viewport register - the GL cube draws the whole frame - so there is no measured value to decode or
  to check a decode against, and the hardware has several candidate registers (`PA_SC_GENERIC_SCISSOR`,
  `PA_SC_VPORT_SCISSOR`, the window and screen scissors) with no capture to say which a guest sets.
  Decoding one from Mesa's layout alone, with no value to verify, is the from-a-document guess the
  register rule warns against, so the frontend does not emit `SetViewport` yet.
- **The backend apply is not blocked.** The `SetViewport(Rect)` command already exists and its meaning
  is unambiguous - "restrict rasterisation to a rectangle" - and it carries the rectangle, so the
  backend needs no register to execute it. That is what this lands.

## What was built

- `SetViewport(rect)` sets the backend's `current_viewport` - frame state, like the render target
  (D703's shape) - rather than being refused by name.
- A draw applies it as the Vulkan scissor. The framebuffer's pipeline already baked a full-attachment
  scissor; now a draw carries an optional one (`Bound::scissor`), and `build_pipeline` uses it,
  **clamped to the attachment** because Vulkan refuses a scissor that reaches past it - a decode that
  went wrong draws a smaller region rather than failing the draw. `draw_vertices` takes it directly;
  the mesh path gets a `draw_mesh_over_clipped` that `draw_mesh_over` is now the no-scissor case of,
  which keeps the shared mesh entry's callers untouched.

## Made to fail

- `a_set_viewport_restricts_the_draw_to_its_rectangle` (device): a fullscreen-triangle draw restricted
  to the left half of the attachment paints the left half green and leaves the right half the clear
  black. A backend that ignored the viewport would paint the whole frame green, so the **black right
  half** is the proof the rectangle took effect - a pass on the green half alone could not distinguish
  the two.
- `a_set_viewport_is_accepted` (no device): a `SetViewport` succeeds rather than being refused,
  touching no device.

## A test the change dated

`an_unimplemented_command_is_refused_by_name` executed a `SetViewport` and asserted it was refused -
true until this unit. Repointed at `ClearColour`, whose execution genuinely has not landed (its clear
value has no register oracle, D702), so the guard still checks that an unimplemented command names
itself.

## What is interim, named as such

- **No frontend emission**, above: `SetViewport` reaches the backend only from a test until a capture
  settles which scissor register a guest sets and what it holds.
- The viewport is applied as a **scissor** (a clip), not as the viewport *transform* (the NDC-to-pixel
  scale and offset the `PA_CL_VPORT_*` registers hold). The command's own contract is "restrict
  rasterisation to a rectangle", which is the scissor; a transform that maps geometry into a sub-rect
  is a separate concept the vocabulary does not carry yet.

## Gate state

`cargo test --workspace` **2412 passed, 0 failed** (+2: one device, one no-device); `cargo clippy
--workspace --all-targets -D warnings` clean; fmt clean; identity scan exit 0. Module status doc
updated - only `ClearColour` remains refused. No commit.
