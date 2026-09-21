# 734. libSceAmpr ruled out as the wall cause: the apr path is not the graphics path

**2026-09-20** — PPSA02664's wall stays blocked on obSCEne `a3f0` (the `sceAgcInit` state buffer). While
it is parked, this tick closed the one upstream hypothesis a prior worklog left open: that the
`libSceAmpr` asset-streaming constructors are what steer the guest into the faulting graphics path.

## The hypothesis, and where it came from

obSCEne `af31` (resolved) answered that `sceAmprAprCommandBufferConstructor` and
`sceAmprCommandBufferConstructor` - the calls that stream `globalgamemanagers.resS` - are non-exports the
title inlines, and named them "the leading candidate for the upstream divergence that steers the guest
into the faulting graphics path (worklog 720)." That framing predates the `sceAgcInit` finding (worklog
731); it was the best guess when the wall was still a bare `0xa8` null with no traced origin.

## Tested, and refuted for the wall

Forcing all three Ampr calls to `0x0` (`ORBISTOUN_RETURN`) changed nothing that matters: the guest still
logs `globalgamemanagers is not considered suitable for apr reads flags:0x0`, still falls back to the
`.res/.resG/.resS` variants, and still faults at the same `read of 0xa8`. So the Ampr constructors are
not the proximate cause of the wall - the report's own "a wall moves" test says so. That is consistent
with what worklog 730-731 established from the other end: the wall is a workload object read out of an
AGC context `sceAgcInit` never filled, which is the **graphics** path, while `libSceAmpr` is the **asset
streaming** path. They are two different subsystems the title runs, and the trace shows the Ampr calls
and the `CreateWorkload` fault sitting on separate flows, not one feeding the other.

The `apr`-suitability decision itself (`flags:0x0`) is also unmoved by the constructors, so whatever
Unity reads to decide a file is not apr-suitable, it is not their return - a separate question, and a
separate divergence if it is one, but not this wall.

## What it settles

One blocker, not two. Before this tick a reader had two open "upstream divergence" candidates -
`libSceAmpr` (from `af31`/720) and `sceAgcInit` (from 731) - and only one is real for the `0xa8` wall.
`sceAgcInit`'s unfilled state is it, and it is waiting on `a3f0`. The Ampr constructors remain honest
placeholders: non-exports with unmeasured writes, which forcing does not help and guessing would only
regress (the lesson worklog 732 relearned on the AGC setters). Their asset-path behaviour may be worth
measuring later for its own sake, but it is not on the critical path to this fault, so it does not
compete with `a3f0` for the next hardware sweep.

## Gate state

No code changed - this tick was `ORBISTOUN_RETURN` diagnostics and trace reading. `./bin/orbistoun check`
was green as of worklog 733 and nothing here touched the tree; identity scan clean. No commit.
