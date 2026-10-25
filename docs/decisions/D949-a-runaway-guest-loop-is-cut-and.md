# D949 - A runaway guest loop is cut and reported

**Status:** decided
**Date:** 2026-10-24
**known_by:** measured

A guest dispatch's module counts every backward jump its invocation takes. Past
`control::LOOP_CAP` (2^20) it leaves the loop for the program's end, as `s_endpgm` does, and
writes the offset of the loop's backward branch into its observation. The dispatch is then refused
like an access escape, naming the program and the loop, and nothing it wrote reaches guest memory.
A module with no observation the host reads for a cut - a draw's shaders, the host-side evaluator -
runs its loops as written.

Where a program is laid out as structure (D972), a backward jump is a loop's back edge: each one
taken counts, the loop's latch names the loop, and a cut leaves through the loop's merge, which
routes the invocation to the program's end. Every cycle a guest can run has a back edge, so the
budget still bounds the invocation's work. A jump to an earlier block that closes no cycle does not
count.

**Why:** a loop that never ends on the host - a wrong translation upstream feeding it a cycle -
runs until the watchdog takes the device, which ends the run and names only the last submissions.
A cut turns it into one refusal line with the loop's offset. One budget for the whole invocation
bounds what the watchdog bounds, the invocation's work, however the loops nest. 2^20 is far above
any loop a guest bounds by its data, and a loop of three scalar instructions run to the cap costs
about 40 ms on an RTX 5070 Ti in either wave model (`a_loop_at_the_cap_runs_and_one_past_it_is_cut`
prints it), so a body some fifty times heavier is still cut inside a two-second watchdog.

**Rejected:**
- A count per loop: nested loops multiply, so it bounds nothing the watchdog cares about.
- Cutting silently and keeping what the dispatch wrote: a wrong answer indistinguishable from a
  right one.
- Ending the run on a cut: one cut loop says nothing about the rest of the frame, and refusing the
  dispatch is what an access escape already does.
- A host timeout on the fence: the device is lost by the time it fires.
