# D982 - A dispatch whose buffers only a host run finds goes device-addressed

**Status:** decided
**Date:** 2026-10-25
**known_by:** measured

Some dispatch buffers can only be found by running work on the host. A computed descriptor is
replayed group by group (D914) or wave by wave over a sample (D951). A dispatch too large to run
whole has its global loads sampled (D950). Where the backend runs device-addressed dispatches
(D964), such a dispatch is prepared device-addressed instead, which finds none of those buffers.
The host run stays only where device addressing cannot run the dispatch.

**The rule.** The rule is decided by what the preparation would do, not by a list of titles. A
computed source that `resolve_range` cannot resolve is not handed to `probed_range`. A global
range of more waves than `MOST_DISPATCH_WAVES` is not sampled. Either is refused with
`HOST_RUN_PASSED`, which says the host cannot know. `prepare_state` then prepares the dispatch
device-addressed (D964) and remembers the program (D970), so its later dispatches skip the trace.
`host_runs_allowed` is true where device addressing is not offered, so every other backend and
every unit test keeps the host run. The memo of traced buffers (D981) keys on it too.

**Where device addressing is refused.** If the device-addressed preparation is refused after a
host run was passed over, the dispatch is prepared again inside `allowing_host_runs`. Only if that
succeeds is the program remembered with `remember_host_runs`, so its later dispatches start there.
A refusal names every route tried, in order: the trace's, the device-addressed preparation's, then
the host run's. `prepare_routed` holds the whole order, and `prepare_state` only says how each route
prepares. Nothing that ran before is refused now.

The rule's first form refused with the host run's reason alone, and remembered the program even
when the host run was refused too. Run 346 then refused `0x74005b854a00` (D758) and
`0x74005b858800` (D753) with no device-addressed part, though both were tried. Named now, each
device-addressed preparation is refused for an image descriptor the host must still resolve:

- `0x74005b854a00`: "fetched image 10: the program branches past the computed image descriptor's
  access at 0x1648".
- `0x74005b858800`: "fetched image 1: a computed image descriptor at 0xbf8 leaves s32 unknown to the
  host, which may differ from group to group (D753)".

Their buffers are not what stops them. An image bound from a descriptor computed per group is the
next wall.

**Queued writes.** A submission's queued writes (D876) ask the same question. They are refused at
the same point and answer `None`: a device-addressed dispatch's writes are not bounded before it
runs. The guest thread no longer runs a sample under the live pipeline.

**Measured** on Grand Theft Auto V. Each cycle of its in-world loop dispatches the program at
`0x74005b87e400` seven times, every 45 to 55 s.

- Host run: 0.6 to 2.4 s a dispatch, growing with its groups (125 to 692). It was paid twice, in
  queued writes and in the command processor's preparation, about 24 s a cycle. D981's memo
  serves the second run in about half the cycles.
- Device-addressed: 1.12 to 1.18 s a dispatch on the device, the same at 125 groups as at 692.
  Nearly all of it is D974's coherence step, not the program. Before each device-addressed
  dispatch the copies' 250 to 265 runs are each marked in the write-watch, at 4 to 8 ms a mark:
  0.8 to 2.1 s a dispatch. "device: run" for the whole cycle's 190 dispatches was 143 ms.
- On the 480 s run, "queued writes" fell from 88 s to 0.36 s, and "live pipeline: wait" from
  172 s to 15 s. "dispatch on device" rose from 69 s to 157 s. The guest still waits about 17 to
  23 s a cycle, now in "queue full" behind the device-addressed dispatches' marks.
- No refusal was added. No device-addressed access escaped.

What is left is D974's marking. Marking every copied run, one write-watch harvest each, before every
device-addressed dispatch of every program costs about a second. That is where the remaining
per-cycle stall is.

**The tests.**

- `a_cannot_know_refusal_is_prepared_device_addressed`: D758's refusal is prepared
  device-addressed, and the next dispatch goes there directly.
- `without_device_addressing_the_host_runs`: without device addressing, only the host route is
  asked.
- `a_refused_device_preparation_falls_back_to_the_host_s_runs`: a refusal names all three routes and
  remembers nothing; a host run that succeeds is remembered.
- `a_host_run_passed_over_goes_device_addressed`: the refusal is one the host cannot know past.

In a 480 s run, D974's copies took 132 s of the 168 s device-addressed dispatches spent on the
device.

**Rejected:**
- A list of programs or titles to prepare device-addressed: the preparation already knows when it
  would run waves on the host.
- Device addressing for every dispatch: one whose buffers resolve from its user data has no host run
  to save, and a bound buffer is written back exactly where a device-addressed store is not watched.
- Keeping the replay for queued writes alone: it would still run under the live pipeline on the
  guest's thread.
