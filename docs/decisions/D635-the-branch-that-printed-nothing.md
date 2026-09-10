# D635 - The branch that printed nothing

**Status:** measured
**Date:** 2026-09-09

## Two states, one silence

D634 found a 44% import regression in PPSA28061 that no instrument reported. The reason is one
line, and it had a comment explaining itself:

```rust
// Nothing to say. The record already holds something as good, which is the ordinary
// outcome of a run that changed nothing.
Ok(Kept::NotBetter { .. }) => {}
```

`NotBetter` covers **two** states. The record holding something *as good* is indeed nothing to say.
The record holding something *better* is a regression - and it printed for neither, discarding the
previous status it was handed in the same pattern.

So a title that lost ground kept its old number in the file, said nothing on screen, and every run
afterwards compared itself to the previous run and reported `same`.

## What it says now

```text
below the best ever recorded for this title: 26 imports and 334 calls, against 47 and 933 on 2026-08-23
reached entered where the record reached entered
this run had 20s against the record's 12s, so time is not the explanation
the record is not overwritten - it is a best-ever, and this run is not one
```

Four lines, and the third is the one that makes it usable. **A shorter run legitimately reaches
less**, so a bare "you reached fewer imports" would cry wolf every time somebody tried a quicker
run - and would be ignored by the third time. Printing both limits settles that reading before
anybody has to go and look, and here it makes the finding *stronger*: twenty seconds against the
record's twelve, and still short by twenty-one imports.

The last line exists because the natural next question - "has it overwritten my record?" - has a
reassuring answer that the reader should not have to hunt for.

## The rule, split out and tested

`is_below_best` is a pure function of two statuses: fewer imports, or the same imports and fewer
calls. The printing is a thin wrapper over it (principle 8).

**Standing is deliberately not consulted.** Implementing a function the guest already called raises
standing while moving no import and no call, so a run can be better on standing and worse on reach
- and reach is what this line is about.

The test asserts all four cases, including both negatives: a run equal to the record must stay
silent, because a line that fires on equality is one people learn to skip, and that is precisely
how the silence it replaces survived. A rule that only ever answered `true` would satisfy any one
assertion alone.

## Watched firing, and watched staying quiet

PPSA28061 prints it. PPSA21564, which is at its own best, prints nothing. Both run before this was
written, because a guard nobody has watched reject something is a guard nobody knows anything
about.
