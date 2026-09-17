# 632. The thin driver, and the first command that actually executes: a compute dispatch runs through the backend

**2026-09-16** - the frame loop that drives a submission through a backend, and the executor's
first executing arm: a bound compute shader dispatched through `RenderBackend::execute`, its
output read back from the device

## The thin driver

`orbistoun_gpu::drive(backend, submission)` runs one submission the way a worker will: make each
module resident, carry out each command in order, then present. It returns a `FrameOutcome`
(`resident`, `executed`, `refused`, `presented`) so a frame is legible from one value. Two rules,
both from honest-failure:

- **A refused command is counted, not fatal.** A backend with an arm it has not grown yet refuses
  by name (D010) and the frame runs to the end of the stream - which is what lets the executor grow
  arm by arm while a real guest keeps submitting. A frame with a high `refused` says which capability
  is missing rather than going black.
- **A residency error ends the frame.** A resource that could not be made resident is one the
  commands cannot reference, so continuing would refuse everything for a reason the caller already
  has. A device error from a command is fatal too - a real failure, not a gap the backend chose.

The loop lives here, not in the backend: the ordering is the frontend's, decoded from the guest's
own stream, so a backend stays a recorder (D701).

## The first executing command

`VulkanBackend::execute` no longer refuses everything. A `BindShader { Compute }` records the bound
shader (an unknown id is a real `UnknownResource`, not a gap), and a following `Dispatch` **runs it
on the device** and keeps the read-back output. The chain the dispatch harness proves - build a
module, create a device, bind a buffer, dispatch, read back - now runs through the backend's own
`execute`, reusing the tested `compute::dispatch`. The graphics-draw commands still refuse by name,
honestly, until pipeline assembly and render targets land.

`VulkanBackend` now keeps a resident shader's SPIR-V words beside its module, so the compute path can
run it; when the executor assembles a pipeline from the resident module directly, the words go.

## What is interim, and named as such

The dispatch's output buffer is a **fixed width** (`DISPATCH_WORDS`) until the buffer arm lands: a
guest's dispatch binds its own buffers, whose sizes come from the guest, but there is no buffer arm
yet, so a dispatch runs a shader that writes its own storage into a buffer of that fixed size and
reads it back. That is enough to execute a *translated shader* end to end through the executor - not
yet enough to run a *guest's* dispatch, which is the buffer arm's job. The interim is documented at
`DISPATCH_WORDS` and in the code, not hidden.

## Made to fail

- `a_bound_compute_shader_runs_on_dispatch_and_reads_back` (gpu-vulkan, device-gated) - a
  `storage_buffer_write_module` writing a known constant is made resident, bound, and dispatched
  through `execute`; the constant comes back from `last_dispatch_output`. Skips where there is no
  device; here there is one, so it is a real end-to-end execution. Fails against a backend that
  refuses the dispatch, or runs the wrong shader.
- `a_dispatch_with_no_bound_shader_is_refused` (gpu-vulkan, no device) - a dispatch with nothing
  bound is refused, not run against an empty shader.
- `a_recording_backend_takes_the_whole_frame`, `refused_commands_are_counted_and_do_not_abort_the_frame`,
  `a_residency_error_ends_the_frame` (gpu, no device) - the driver's three behaviours: a frame that
  all lands, a frame all refused (counted, not aborted), and a residency error that stops it.

## Next, under the same seam

The `Buffer`/`Texture` arms (so a guest's dispatch binds its real buffers and the fixed width goes),
then the graphics path (pipeline assembly from resident modules + render targets). None changes the
`ensure_resident`/`execute`/`present` interface (D701).

## Gate state

`cargo test --workspace` green; `cargo clippy --workspace --all-targets -D warnings` clean; fmt
clean. Five new tests, two of them device-gated (one a genuine GPU execution).
