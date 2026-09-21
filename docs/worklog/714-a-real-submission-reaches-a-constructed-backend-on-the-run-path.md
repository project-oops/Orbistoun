# 714. A real submission reaches a constructed backend on the run path

**2026-09-19** — inbox `-36c0`: G8-G12 were verified against material generated here, with no path by
which a real guest submission could reach a graphics backend. `sceAgcDriverSubmitDcb` walked a
guest's command buffer through the translator and pipeline but, as its own comment said, "reports
rather than renders - no backend is attached": it kept the `SubmissionReport` and discarded the
submission's commands. This attaches the backend, on the worker's side of the D695 seam.

## The seam D695 already drew

D695 decided the **worker** renders headless and hands the shim the bytes - `orbistoun-gpu` keeps no
dependency on a graphics runtime (principle 12), so the device is opened one layer up. So the change
is split exactly there:

- **`orbistoun-gpu`** (`agc_driver`): the submit handler now keeps the whole `Submission` - commands
  and modules, not only its report - behind `take_last_submission()`. `last_submission_report()`
  still answers the report side for the run summary, derived from the same whole.
- **`orbistoun-worker`** (new `render` module): after the guest returns, `render_and_log_last_submission`
  takes that submission, probes for a device, and if one is present constructs a `VulkanBackend` and
  drives the submission's commands to it, logging the outcome (device, commands driven, refused,
  frame). No device is a fact it logs, not a failure - a run on a machine without a GPU still reports
  its reach and imports. The worker gains an `orbistoun-gpu-vulkan` dependency for exactly this; the
  graphics crate stays clean.

## What reaches the backend, measured

`a_captured_submission_reaches_a_constructed_backend` drives the console triangle's real command
buffer - the same one f50b renders - through `render` to a device the worker opens itself. On an
NVIDIA RTX 5070 Ti its commands reach the backend (`reached_backend()`), which is the seam `-36c0`
exists to open; it skips where no device is present, the way every device test does. A second test
pins the `reached_backend` rule: a device **and** a command, with a refusal counting as arrival.

## What this does and does not claim

- **Established, not yet exercised by a title.** No corpus title reaches `sceAgcDriverSubmitDcb`
  today - they fault earlier - so `render_and_log_last_submission` is a no-op on every real run now.
  It builds the route; the moment a title submits, its command buffer reaches a real backend instead
  of a report. The test drives the route with a captured submission, which is the strongest handle
  available until a title gets there.
- **Wired on the ordinary-return path.** The call sits where a guest entered as a function returns
  (D153). A title entered as a process (the common path) faults or hits the time limit and never
  returns here; those two trace-persistence points gain the same one-line call when a title actually
  submits under them - deferred deliberately, because none does yet, rather than threaded blind.
- **Last submission, not every frame.** One render of the last command buffer after the run, which is
  what the acceptance needs; per-frame rendering into the `frame_region` route (7f1b) for the GUI is
  the next step, not this one.

## Gate state

`./bin/orbistoun check` passes end-to-end (all checks passed): the workspace compiles, clippy
`-D warnings` clean, the new render tests and the device triangle/point renders pass, cargo-machete
accepts the new `orbistoun-gpu-vulkan` and dev `orbistoun-translate` deps, docs/prose/fmt clean,
identity scan clean. No commit.
