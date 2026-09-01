# orbistoun-gpu-vulkan

Vulkan implementation of `orbistoun-gpu`'s `RenderBackend`.

**Models:** nothing yet. Every command is refused with `Unsupported`.

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

**Status:** the rendering backend is still a stub - every draw and present is refused by
name. Roadmap phase 6.

What is *not* a stub is `compute`: a real Vulkan device, dispatching a translated shader
with known inputs and reading the buffer back, so a translation can be checked against
what it was supposed to compute rather than merely validated as well-formed. That is why
this crate depends on `ash` now, having deliberately not done so while nothing used
Vulkan (D019).

A missing device is reported as a missing device, never as a pass. A suite that finds no
GPU, returns early and goes green is a suite where the most important test never ran.
