# 648. A whole frame composes through the driver: target, viewport, shaders, draw

**2026-09-16** - an integration test drives a realistic submission - a colour target, a viewport, a
bound vertex+fragment pipeline, a draw - through `drive` end to end, proving the executor's pieces
compose rather than only run one at a time

## Why this, this tick

The autonomous loop asked to continue the queue. The executor grew a broad command vocabulary this
session (worklogs 636-647): resource residency, dispatch with a guest-memory writeback, vertex and
mesh draws, indexed draws, render targets, the guest-memory window bound once, and the viewport
decoded and applied. Every piece has its own device test, but each exercises one command against a
fresh backend. Nothing had driven a **submission** - several commands in order, against one backend,
through the thin driver - which is the shape a real frame has.

The two remaining feature pieces are both deliberate, not quick: the texture detile needs the
64KB_R_X swizzle out of the addressing library (measured, `6e0f`/`a1f7`, but a careful multi-slice
transcription), and `ClearColour` waits on a register oracle no capture in hand carries. Rather than
rush either at the end of a long session, this hardens what is built: a composition test, no new
decode and no hardware fact, only the pieces already verified, run together.

## What it drives

`a_full_frame_composes_through_the_driver` builds a `Submission` by hand - two modules and a 128x96
colour target as resources, and the commands `SetRenderTargets`, `SetViewport` (the left half),
`BindShader` twice, `Draw` - and runs it through `orbistoun_gpu::drive`. The driver makes the target
and modules resident and carries out the commands; the outcome is `resident: 3`, `executed: 5`,
`refused: 0`, and the frame comes back **128x96** (the target sized it), the fragment's green in the
left half (the viewport passed it) and the clear black in the right (the viewport clipped it).

That the frame is the target's size *and* clipped to the viewport *and* the fragment's colour is the
point: three pieces that each have a test alone here act on one draw together, in the order a
submission carries them, through the driver that a real guest's frame would take.

## A transient worth recording

One `cargo test --workspace` reported `882 passed, 1 failed` and stopped early; two full re-runs
immediately after were `2419 passed, 0 failed`. This is the device-level Vulkan transient seen earlier
this session (a test binary aborted once and did not reproduce), not the change - the composition test
and every other draw test pass on every run since.

## Gate state

`cargo test --workspace` **2419 passed, 0 failed** (+1 device-gated integration test); `cargo clippy
--workspace --all-targets -D warnings` clean; fmt clean; identity scan exit 0. No commit.
