# D699 - int 0x41 is measured fatal, so it reclassifies from kernel entry to guest trap

**Status:** decided
**Date:** 2026-09-15

## The choice

obSCEne measured `int 0x41` on the device (REQ-...b3c2, sweep `20260915-174357`, rows 2277-2282): a
bare `int 0x41` from userspace on retail, at selectors `0x0` and `0x1`, raises a signal (`0xa`) and
does not return. The choice is what orbistoun's diagnosis does with that, and it is to **reclassify
`int 0x41` from `KernelEntryUnimplemented` to `Faulted`** - a guest trap whose cause is upstream - and
to leave every other trap vector where it was.

## Why reclassify rather than add a handler

The `KernelEntryUnimplemented` class (worklog 605) was built on the hypothesis that `int 0x41` is a
kernel service orbistoun had not yet implemented, waiting on a device measurement to say what it reads
and returns. The measurement refutes the hypothesis: there is no returning service behind `int 0x41`
on this platform. So the honest classification is the one the `ud2` path already has - a trap the
guest reaches on a failed path, whose cause is the wrong value it was handed just before, not the
trap. A retail title that runs on hardware (PPSA04263, byte for byte) never executes this `int 0x41`
there; under orbistoun it does, because an upstream orbistoun error sent it down that path.

Keeping it as a kernel entry would keep telling the reader to "characterise it, then add the handler"
- advice the measurement has proven leads nowhere, and the precise plausible-output the project
forbids at the tooling level (principle 3): a report claiming a next step its own evidence refutes.

## Why only int 0x41, and not all `int` vectors

Because only `int 0x41` was measured. Every other vector - other `int n`, `syscall`, `hlt` - is still
unmeasured and may genuinely be a service orbistoun should implement. Peeling off the whole class on
one vector's measurement would be the inverse error: discarding a real "characterise this" next step
for vectors nobody has asked the device about. The split is drawn at exactly the evidence: the one
vector a measurement settled moves; the rest keep the job.

## The consequence, which is a better next step rather than only a truer label

`Gap::Faulted` routes (orbistoun-turn) to `SweepArguments` on the call leading into the trap - the
one-bit oracle the loop already uses - where `KernelEntryUnimplemented` routed to `Person` ("file a
device measurement"). The measurement being in turns a dead-end into a loop step: find the upstream
value orbistoun fed the guest by sweeping the gate call, the same move worklog 610 named for a
give-up. Three surfaces that used to say "characterise + add the handler" - the ranked finding, the
dispatch, and the worker's live crash print - now agree it is a measured-fatal trap pointing upstream.

## What did not change

The interrupt-dispatch mechanism (worklog 608, D-none) stays: it is correct infrastructure for a
vector that measures as a real returning service, and `int 0x41` simply is not one. Its table stays
empty for `0x41` - now because the measurement says there is nothing to register, not because the
measurement is pending.
