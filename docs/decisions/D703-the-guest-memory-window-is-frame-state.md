# D703 - The guest-memory window is frame state the driver sets, seeded per draw, not a bound resource yet

**Status:** decided
**Date:** 2026-09-16

## The choice

A guest's geometry shader fetches its vertices from the guest-memory window - the flat `GUEST_MEMORY`
storage buffer (binding 1) every translated module reads, bounded by the window the module is
compiled against (worklog 635). The executor's draws bound an *empty* window, so a guest's shader
would read zeros. This decides how the window reaches the backend and how it binds. Two decisions,
both deliberately the simpler of the available shapes:

1. **It reaches the backend as frame state, through `RenderBackend::set_guest_memory(&[u32])`, not as
   a command or a per-slot bind.** Every module in a submission shares the one window; a shader does
   not name it, the translation compiled it in. So it is set once per frame - the frontend reads the
   window's span out of guest memory (`read_guest_window`) and carries it on the submission, and the
   driver hands it to the backend before the commands. A `RenderCommand::BindBuffer` was the
   alternative, but its `slot` is *the guest's* numbering, and the guest-memory window is a
   translation artifact at a fixed binding the guest never numbered - so binding it through that
   command would misrepresent what it is.

2. **The backend seeds it into a buffer the draw owns, per draw, rather than binding a resident
   buffer.** The tested graphics path (`framebuffer::draw_mesh_over`) already fills binding 1 from a
   `&[u32]` and reads it back, so seeding reuses a verified path. Binding a *resident* buffer directly
   at binding 1 - the graphics counterpart of compute's `dispatch_into`, "upload once" - is the
   better end state, but it needs `build_pipeline`/`release` to hold a buffer they do not own, which
   is delicate ownership surgery in the harness. Seeding does not *constrain* that later change
   (principle 11): the frontend extracts the same bytes either way, so the direct-bind is a backend
   optimisation, not a redesign. Taken now: seed; deferred: direct-bind.

## Why the window's exact span, and empty when unmapped

`read_guest_window` reads exactly `window.words()` words from `window.base`. That length is the mask
the module applies (`Window::spanning` refuses a non-power-of-two precisely so the mask is the bound),
so a backend that bound a region of a different length would fold an index to a different word than
the shader meant. A region that is not fully mapped reads **nothing**, not a partial or zero-filled
window: the default window sits at address zero and is unmapped, so a stream that never placed its
window carries an empty region rather than a page of whatever is at zero - the honest answer, and
what a shader reading an unplaced window would find.

## What this is verified against, and what it is not

Verified end to end by reading the window **back** through the executor: seeded with known words and
drawn with a mesh that ignores the window, it comes back the seed - proof it was bound and carried the
guest's bytes rather than zeros. It is **not** yet verified that a guest's shader *reads* the seeded
window and renders from it, because no hand-assembled fixture reads the window - only a real
translated guest shader does, and running the captured frame through the executor also needs the
texture path, which is not built. So this lands the window's delivery and binding, and the captured
frame that reads it is a later unit.

## What is committed and what is open

Committed: the guest-memory window is frame state, extracted by the frontend to its exact span, set
on the backend by the driver, seeded into the draw. Open (named, not built): binding a resident
buffer directly (upload once); the compute path's own guest-memory writeback (worklog 635), which
this leaves untouched; and the captured frame that fetches vertices from the window, which needs the
texture path too.
