# orbistoun-gpu-vulkan

The Vulkan implementation of [orbistoun-gpu](../orbistoun-gpu/)'s `RenderBackend`.

It makes resources resident and executes against a real Vulkan device: compute dispatch,
geometry and fragment draws (mesh or vertex, following the bound module's execution model),
render-target selection, shader and buffer binding, and viewport. Every dispatch and draw is
read back. [orbistoun-worker](../orbistoun-worker/) drives it.

## Rules

- **Refusal by name.** A command the backend does not execute is refused with
  `BackendError::Unsupported`, naming the command (D010). A backend that returned `Ok` and
  drew nothing would be indistinguishable from a rendering bug.
- This is the only crate that names a graphics API. `orbistoun-gpu` has no `ash` dependency,
  so host-API concepts cannot leak into the translator; `cargo` enforces the boundary, not
  code review. A second backend is a sibling crate.
- A missing device is reported as a missing device, never as a pass. A suite that finds no
  GPU, returns early and goes green is a suite where the most important test never ran.

## Checks

`refused()` counts refused commands. Before anything renders, "the translator emitted
nothing" and "the translator emitted plenty and none of it landed" look identical from a
black screen; the counter separates them.

The `compute` path dispatches a translated shader with known inputs on a real device and
reads the buffer back, so a translation is checked against what it was meant to compute
rather than only validated as well-formed. A draw reads its frame back the same way.
