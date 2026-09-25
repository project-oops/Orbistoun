# 814. A submission reaches the backend on the endings a stalled guest actually takes — and the cube's clear self-test walks to zero packets, so the first blocker to execution is that orbistoun cannot see the command buffer, not speed

**2026-09-24** — worklog 813 left both fully-owned baselines waiting on a fence that only an executed
submission can write (D705). Before building execution at submit time, one number was needed: what
does driving a real submission through the backend cost, and how much of it does the backend carry
out?

## The render step now runs where a submitting guest ends

`render::render_and_log_last_submission` drove the last submission to the Vulkan backend only on the
**ordinary return**. Its own comment said the fault and time-limit paths would gain the same call
"when one does" — when a title submits. Both baselines now submit and then wait out the clock or the
budget, so neither ever returned and nothing was ever driven. The time-limit and call-budget endings
now call it, and the log line carries how long the drive took.

**And the ordinary return had it in the wrong order.** Rendering *takes* the submission out of its
slot, and the trace's submission summary (`report::submission_summary`) reads that same slot when the
trace is collected — so rendering first left the summary empty every time. All three endings now
collect the trace first and render after.

## What the cube's submission is

```
orbistoun: a submission reached the NVIDIA GeForce RTX 5070 Ti backend: 0 command(s) driven, 0 refused, in 154 ms
  ! the guest submitted a command buffer: 0 packets, 0 draws, 0 shader candidates
```

The drive is cheap — 154 ms including opening the device — so time is not what stands between a submit
and its execution. **The submission is empty.** `submit_described` reads the descriptor and then
refuses a command buffer lying outside every region the guest was given, recording an empty
submission rather than dereferencing it (5bff). That is what happened here: the GL context's own
command buffer, which it built, flushed and handed over, is not in the region list the submit reads.

The regions come from `set_guest_regions`, which the worker calls **before entering the guest**, from
the memory map at that moment. The GL context allocates its command buffer during the run, from direct
memory it maps after entry (`sceKernelAllocateMainDirectMemory`, `sceKernelMapDirectMemory`, fourteen
and thirteen calls on the cube). The likely reading is that the region list is a snapshot that never
learns of a later mapping — **not yet confirmed**: the descriptor's `gpu_addr` is not logged, and
checking it against the mappings the run made is the next unit. If it holds, it is the fix every
submission needs before any of it can execute, and it would equally have emptied every retail title's
submission had one got this far.

## Gate state

`crates/orbistoun-worker/src/render.rs` (timing, the doc), `report.rs` (the two endings),
`lib.rs` (the ordering on the ordinary return). A reporting change: the guest runs as in worklog 813.
`./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
