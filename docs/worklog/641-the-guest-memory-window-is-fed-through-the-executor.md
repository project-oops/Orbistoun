# 641. The guest-memory window is fed from guest memory through the executor

**2026-09-16** - the second half of the geometry-source arc: a draw's guest-memory window is read out
of guest memory by the frontend, handed to the backend, and seeded into the draw, so a guest's
geometry shader has vertices to fetch rather than the zeros an empty window gave

## What this joins

Worklog 640 taught the executor to draw a guest's geometry stage - a mesh shader - but through an
**empty** window, so a shader that fetches its vertices from guest memory read zeros. This feeds it.
Every translated module reads guest memory through one flat window the whole submission is compiled
against (worklog 635); the pieces that carry it:

- **Frontend** (`read_guest_window`): reads exactly the window's span - `words()` words from `base`,
  the length the module masks against - out of guest memory, and carries it on
  `Submission.guest_memory`. An unmapped window carries nothing, not a zero page.
- **Trait seam** (`RenderBackend::set_guest_memory`): frame-level state, not a command and not a
  per-slot bind, because a shader does not name the window - the translation compiled it in (D703).
  The default ignores it; a recorder records it.
- **Driver**: hands the frame's guest memory to the backend before the commands, since a draw reads
  it.
- **Backend**: seeds a mesh draw's window (binding 1) from it through the tested `draw_mesh_over` path
  and reads it back as `last_window()`.

## Verified, and the honest limit of the verification

Read **back** through the executor: `a_mesh_draw_binds_and_reads_back_the_guest_memory_window` seeds
known words, draws a mesh that ignores the window, and the window comes back the seed - proof it was
bound and carried the guest's bytes, not the zeros an unfed window gives. The frontend
(`the_guest_memory_window_is_read_from_guest_memory`: exactly the span, empty when unmapped) and the
driver (`the_guest_memory_window_reaches_the_backend`) halves are pinned with no device.

What is **not** verified: that a guest's shader *reads* the seeded window and renders from it. No
hand-assembled fixture reads the window - only a real translated guest shader does - and running the
captured frame through the executor also needs the texture path, which is not built (D703). So this
lands the window's delivery and binding; the captured frame that fetches from it is the next unit.

## The two design calls (D703)

- **Frame state, not a command.** The window is not guest-numbered - it is a translation artifact at a
  fixed binding - so `set_guest_memory` sets it once per frame rather than a `BindBuffer` misusing its
  guest `slot`.
- **Seed per draw, not bind a resident buffer yet.** Seeding reuses the verified `draw_mesh_over`
  path; binding a resident buffer directly at binding 1 ("upload once") is the better end state but
  needs delicate ownership surgery in the harness, and seeding does not constrain that later change
  (principle 11).

## Gate state

`cargo test --workspace` **2406 passed, 0 failed** (+3: frontend, driver, device+mesh); `cargo clippy
--workspace --all-targets -D warnings` clean; fmt clean; identity scan exit 0. D703 written. No
commit.
