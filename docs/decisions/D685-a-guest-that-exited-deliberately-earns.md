# D685 - A guest that exited deliberately earns a rung, below a flip

**Status:** decided
**Date:** 2026-09-14

## The choice

`Reach` gains `Exited`, between `Entered` and `Flipped`:

```
Rejected -> Parsed -> Linked -> Entered -> Exited -> Flipped
```

A guest reaches it by leaving through `exit`. A guest that presented a frame **and** exited is
recorded as `Flipped`; the deliberate stop is still carried in `Status::outcome`.

## Why it earns a rung at all

D182 refused a rung for surviving to the time limit, because *not dying is an outcome, not a
distance* - a guest spinning on four unimplemented functions survives. Exiting is the other shape,
and it passes the same test D558 used to admit `Flipped`: a guest reaches it by **a call it made**,
with a status it chose, at the end of the work it set out to do. There is no way to spin into it. It
is a specific thing done, not a thing not happening.

It also fixes a real mis-reading. Before `_exit` was bound (worklog 543) the conformance payload fell
off its entry point and was recorded as faulting at `0x5e2d`; afterwards it stops deliberately, and
the ladder had no way to say that was better.

## Why it sits below a flip, which is the whole decision

`Status::ranking_key` compares the rung **before** imports, answered, standing, frames or calls. A
rung therefore dominates every quantity beneath it, so admitting one that is *trivially reachable* is
how a ladder starts lying - which is exactly what D182 and D558 record happening twice already.

Exiting is trivially reachable. A program whose first instruction is `exit(0)` reaches it having
learned nothing. Ranked above `Flipped` it would sort that run above a title rendering frames, and on
this corpus the effect was concrete rather than theoretical: the conformance probe would have moved
from fifth to **first** on the frontier, above `PPSA99980`, `PPSA03416` and `PPSA02664` - the "closest
to running" list headed by our own test harness.

A frame is the harder thing. It is accepted only after the guest has opened an output, set its
attributes, registered buffers and configured it, each against a real implementation (D558). So the
frame keeps the higher rung and finishing sits just below it.

## What this deliberately does not fix

The case that prompted it. `obscene-payload` both flips and exits, so it stays at `Flipped`, and the
`BACK` verdict that made a correctness fix look like a regression is unchanged - its import count
genuinely fell by one when it stopped running past its own `exit`.

That is a separate problem: at equal reach, the record has no way to say *this run was better in a
way the score does not capture*. Naming it here rather than solving it, because the fix for it is a
change to what `beats` compares, and that wants its own evidence.

## The guards

Three tests, and the third is the one that matters:

- `a_deliberate_exit_outranks_having_only_entered`
- `a_frame_outranks_a_deliberate_exit`
- `a_trivial_exit_does_not_outrank_a_title_that_rendered` - asserts the ordering this decision turns
  on, so moving the variant up the enum fails here rather than quietly reordering the frontier. It
  was checked by moving it: two tests fail, naming the case.

`giving_up_does_not_earn_the_rung` pins the other half - `abort`, an unhandled signal and the time
limit all stay at `Entered`. Stopping is not finishing.

**One coupling, named and guarded.** `orbistoun-report` awards the rung by matching the worker's
wording for a deliberate stop and cannot depend on `orbistoun-core` to reference the enum producing
it. The string is a named constant there, and a test in `orbistoun-worker` - the one crate that sees
both - asserts they agree. A drift would not break anything loudly: the rung would simply stop being
awarded and every run would fall back to `Entered`, which is precisely the silent mis-measurement
principle 3 exists to refuse.
