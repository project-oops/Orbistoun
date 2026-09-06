# D561 - The per-thread scheduling record D523 said would be needed

**Status:** guest-observed
**Date:** 2026-09-04

## The prediction, and its arrival

D523 accepted `scePthreadSetaffinity`, dropped what it was given, and said exactly why that was
tolerable and exactly when it would stop being:

> orbistoun keeps no per-thread scheduling record - so the mask is accepted and dropped. **The
> moment something does, this needs the per-thread record rather than a wider `Ok`.**

Something does. PPSA02664 calls `scePthreadGetschedparam` **34 times**, and a get is a read-back:
a setter that dropped what it was given would answer with a value the guest never set. So
`ThreadRecord` gains a policy and a cancellation state beside the priority it already carried.

## What the guest fixed, and what it did not

Arities came from the run, the same way D516, D523 and D524 got theirs - the register holding
`0x7fff_0001`, orbistoun's own placeholder, marks where the arguments stop:

| Call | Arity | From |
|---|--:|---|
| `scePthreadGetschedparam(thread, policy, param)` | 3 | already recorded; the run agrees |
| `scePthreadSetschedparam(thread, policy, param)` | 3 | handle, `0x4000`, a stack pointer |
| `scePthreadSetprio(thread, priority)` | 2 | handle, `0x100` |
| `scePthreadRename(thread, name)` | 2 | handle, a pointer |
| `pthread_setcancelstate(state, oldstate)` | 2 | `1`, a stack pointer |

**`scePthreadAttrGetstackaddr` was left alone**, and that is the decision worth recording. Its
`arg0` and `arg1` in the run are **byte-for-byte identical to `scePthreadAttrGet`'s**, and its
`arg0` is a *thread* handle where an attribute object belongs. Nothing here explains that;
D527 is this project mistaking a register that survived a tail-jump for an argument, and one
unexplained coincidence is not enough to implement against.

## The four-byte rule, in the place it bites

The guest passes `policy` at `0x…c86c` and `param` at `0x…c868` - **four bytes apart**, because
a `sched_param` is a single `int` and a caller putting both on its stack puts them adjacent. An
eight-byte write to either destroys the other, and the guest would then read a policy it never
set with nothing in any trace to say why.

That is D272's lesson arriving somewhere unmissable, and it is guarded with a sentinel either
side rather than by inspection.

## The policy is stored and not interpreted

PPSA02664 passes `0x4000`. That is no POSIX policy constant - those are small integers - so it is
a vendor value, nothing here knows what it selects, and nothing here pretends to. Storing it
verbatim is what lets a get hand back what a set was given, which is the whole of what a setter
promises. **Neither policy nor priority is applied**: which host thread runs when is the host
scheduler's business, exactly as `scePthreadAttrSetaffinity` already records for affinity.

## What it changed

| | before | after |
|---|--:|--:|
| unimplemented functions called | 35 | **26** |
| calls landing on a stub | 914 | **32** |

## The problem this exposes in the record

**None of it moves a recorded metric.** `Status::standing` is an integer percentage of calls that
reached an implementation, and 914 stubbed calls out of 419,091 already rounded to 100% - so does
32. Reach, imports and frames are unchanged. A 96% reduction in calls resting on placeholders is
invisible to the compatibility record, which is a gap in the record rather than in the work, and
is recorded here rather than fixed in the same change.

## What this does not establish

**That any of it is correct**, only that it round-trips. A get hands back what a set stored; if
the vendor's default policy is not zero, a guest reading one before setting one gets a value no
console would give it. Zero is written as *"nobody said"* because no lawful source here gives the
default, and that is the assumption most likely to be wrong.

**Nor that the priority is a plain `int`.** POSIX's `sched_param` is, and the guest's own spacing
agrees, but a vendor field past it would sit beyond what this writes and read as whatever the
caller left on its stack.
