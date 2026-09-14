# D686 - A deliberate exit outranks a fault, above the import count

**Status:** decided
**Date:** 2026-09-14

## The choice

`Status::ranking_key` gains how the run ended, **immediately after the rung and above `imports`**:

```rust
(reach, exited_deliberately, imports, answered, standing, frames, calls)
```

A run that left by calling `exit` beats one that did not, at equal reach, whatever the import counts
say.

## Why above `imports`, which that function otherwise forbids

The rule beside `ranking_key` is explicit: *everything after `imports` is a quality measure and must
stay below it - a guest that gets further calls more, and some of what it calls will be
unimplemented, so any of these ranked higher would report going further as going backwards* (D182,
D558). This is the one exception, and it is deliberate.

The case it exists for is a run whose import count fell **because it stopped correctly**. Binding
`_exit` (worklog 543) made the conformance payload leave through `exit` instead of running off its
entry point and faulting at `0x5e2d`. It then stopped making six calls and touching one import that
it had only ever reached by running past its own refused `exit` - so the metric read a correctness
fix as a regression and reported `BACK`.

Those six calls were never progress. The metric was counting the program falling off the end of
itself. A tiebreaker *below* `imports` cannot fire for that case, because `imports` is exactly what
moved, so below the line it would have been a tiebreaker for nothing.

**Verified on the guest**: the payload's verdict went from `BACK - reaching less of the interface
than it did` to `same - nothing moved`.

## The cost, stated rather than hidden

On this corpus **every game faults and neither of our own guests does**. So an ending ranked above
`imports` systematically sorts the conformance probes above the titles: once a record carries a
deliberate exit, `obscene-payload` at 187 imports outranks `PPSA02664` and `PPSA03416` at 220, which
faulted.

That is the same shape as the objection that kept `Exited` below `Flipped` in D685, one field down,
and it was accepted here where it was refused there for one reason: **the rung is reachable by a
trivial program, and this tiebreaker is not the thing that decides the rung.** A guest still has to
reach the same rung before the ending is consulted at all, so a program that exits immediately is
compared at `Exited` and loses on imports to everything else at that rung. What it cannot do is
overtake a title that flipped.

It has no effect on the committed frontier today, because no record carries a deliberate exit yet -
the payload's `BACK` runs were never written. The first re-run that records one will move it, and
that diff is what `the_frontier_matches_what_is_committed` exists to put in front of a reader.

## What is still not fixed

The payload's run is now `same`, not better, so it still does not overwrite its record: `beats`
requires strictly greater and nothing else moved. The stored record remains the four-day-old
`0x5e2d`. Making a run record on "different and not worse" is a separate question about what a
record is for, and is not decided here.

## The guards

- `a_deliberate_exit_beats_a_fault_at_equal_reach` - the decision as a test, built on the real
  shape: equal reach and frames, one import and six calls fewer, a fault at `0x5e2d` on the other
  side. Moving the field below `imports` fails it with *stopping correctly must not read as a
  regression*; checked by doing it.
- D685's `a_trivial_exit_does_not_outrank_a_title_that_rendered` still passes, which is what pins the
  boundary between the two decisions.

`DELIBERATE_EXIT` moved from `orbistoun-report` to `orbistoun-overrides`, because `Status` now has to
recognise it too: a run that flipped *and* exited is recorded as `Flipped`, so the rung alone cannot
say how it ended. The drift guard moved with it and is now a test-only dependency in
`orbistoun-worker`, still the one crate that sees both it and `orbistoun-core`.
