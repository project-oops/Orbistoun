# orbistoun-gpu-vulkan

Vulkan implementation of `orbistoun-gpu`'s `RenderBackend`.

**Models:** compute dispatch, geometry and fragment draws (mesh or vertex), render-target
selection, shader and buffer binding, and viewport - each executed against a real Vulkan device
and read back. `ClearColour`, `Fence` and `present` are still refused with `Unsupported`.

**Deliberately fakes:** nothing. A backend that returned `Ok` and drew nothing would
be indistinguishable from a rendering bug, so refusals are explicit and name the
command that was refused (D010).

**Design note.** This is the only crate in the workspace that names a graphics API.
`orbistoun-gpu` has no `ash` dependency, so host-API concepts cannot leak into the
translator - `cargo` enforces that boundary, not code review (CLAUDE.md principle 12).
A second backend is a sibling crate, not surgery.

`refused()` exists for a specific early diagnostic: before anything renders, "the
translator emitted nothing" and "the translator emitted plenty and none of it landed"
look identical from a black screen. The counter separates them.

**Status:** draws and dispatches execute against a real device and the frame is read back;
`ClearColour` (awaiting its register oracle, D702), `Fence` and `present` are still refused by
name (D010). Roadmap phase 6 finishes it.

The tightest check is `compute`: a real Vulkan device, dispatching a translated shader
with known inputs and reading the buffer back, so a translation can be checked against
what it was supposed to compute rather than merely validated as well-formed - and a draw reads
its frame back the same way (D701). That is why
this crate depends on `ash` now, having deliberately not done so while nothing used
Vulkan (D019).

A missing device is reported as a missing device, never as a pass. A suite that finds no
GPU, returns early and goes green is a suite where the most important test never ran.
