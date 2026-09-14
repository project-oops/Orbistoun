# D687 - A record updates on not-worse-and-different, not only on better

**Status:** decided
**Date:** 2026-09-14

## The choice

`keep_status` gates on a new `Status::worth_recording` rather than on `Status::beats`:

```rust
comparable_with(previous)
    && key >= previous.key
    && (key != previous.key || outcome != previous.outcome)
```

`beats` is unchanged and keeps its meaning - *is this an improvement* - which is the right question
for a verdict and the wrong one for a record.

## Why the two questions are different

A run can be equal on every ranked field and still carry something the record should hold. The
clearest case is where it ended: a guest that reaches precisely as far but dies at a **different
address** has not improved, so `beats` is false, and the record goes on naming a fault site the guest
no longer reaches for as long as nothing else changes. The file then describes a run nobody can
reproduce, which is the failure D182 built the record to avoid.

"Not worse" is the right bar rather than "better" because a record is a description, not a trophy.

## Why an identical rerun still writes nothing

`measured_on` differs on every run by construction, so "different" deliberately excludes it - counting
it would make every rerun worth recording and delete the rule. A record that churns on every run is
one nobody reads diffs of, and the frontier test exists precisely to make those diffs worth reading.

## What prompted it, and what it did not fix

The conformance payload's record has said `outcome = "0x5e2d"` since 2026-09-10 while every recent run
stops deliberately, and this was expected to correct it. **It does not**, and the reason is worth
recording because three separate diagnoses were wrong before the right one:

The payload's recent runs produce **zero frames**, so they reach `Exited`; the stored record has
**eight frames** and reaches `Flipped`. A lower rung is genuinely worse, and the record is right to
keep the better one. Nothing about the recording rule was blocking it.

That leaves a real finding underneath: **the payload used to flip and no longer does**, and the
retained record hides it. Not chased here.

## What was actually wrong with the reporting

The refusal said *"the status entry is better or equal"* for three different situations - better,
equal, and **not comparable at all** (a different stub policy short-circuits `beats` before any
number is read). A reader told "better or equal" about an incomparable pair has been told something
false. The wording is now "better, or the same run again", and the incomparable case remains
conflated - named here so the next person to touch it knows it is still there.

Worse: on the `NotBetter` path the CLI prints **nothing at all** unless the run is below best, so a
run that was compared and refused looks identical to a run that was never compared. That cost most of
an investigation today: three wrong diagnoses - that `beats` was too strict, that the guest never
reached the recording step, and that the stop reason was not persisted - all because the silent path
looks like the absent one. Not fixed here; it wants its own change.
