# D563 - The record could not see the work, and three copies of the ordering had drifted

**Status:** measured
**Date:** 2026-09-04

## The gap

A day's work took the functions PPSA02664 called and did not get from **35 to 20**, and the calls
landing on placeholders from **914 to 32**. The compatibility record showed none of it.

`Status::standing` is an integer percentage of *calls* that reached an implementation. 914 stubbed
out of 419,091 is 0.22%; 32 out of 418,464 is 0.008%. **Both round to a standing of 100.** Reach,
imports and frames were unchanged, so the whole day was invisible to the only record this project
keeps.

## Why not more precision

Because precision is not the problem. **A percentage of calls belongs to whatever the guest loops
on** - PPSA02664 spent 12,924 calls in a single wait - so it swings on where a title happens to
spin while the thing a person acts on barely moves. Storing it in basis points would have made a
meaningless number more exact.

The measure that moved is **distinct imports called with nothing behind them**. It counts
functions, it is stable against a hot loop, and it is literally the work list: the number of
things the guest asked for and did not get. Recorded as `unanswered`, ranked as `answered`
(`imports - unanswered`, so more is better and it composes with the rest of the key).

## Where it ranks, and the trap it avoids

Below `imports`. This is D182's shape for the third time: **a guest that gets further calls more
imports, and some of what it newly calls will be unimplemented** - so a run can legitimately reach
further and have *more* unanswered than before. Ranked above `imports`, going further would report
as going backwards. Guarded, and the guard was watched failing with the two swapped.

## Optional, because the old records cannot answer

`None` means *this run did not measure it*, which every record written before today is. It is not
`Some(0)`. Collapsing the two would let a stale record claim a perfect score it never earned and
then refuse every honest run that followed - the exact trap `propping` sprang this morning, when
33 records deserialised as honest and made a new run incomparable (D557).

An unmeasured record ranks as though nothing was answered. That is not a claim that nothing was;
it is a record that cannot say, ranked where it can do no harm, and it self-heals the next time
that title runs.

## The bug found on the way, which is the more important half

**Three copies of the ordering existed and two had already drifted.** `Status::beats` decides what
gets recorded, `frontier` decides what a shim shows, and `render_markdown` decides the table -
each held its own tuple. Neither of the latter two gained `frames` when D558 added it *this
morning*, so the new rung ranked one way in the record and another in the table.

`frontier`'s own documentation had already warned about precisely this:

> a table that disagreed with the thing deciding what to record would be the more convincing of
> the two and the wrong one

It was right, and the answer was not three careful edits. There is now one `ranking_key`, used by
all three, and a guard asserting the record, the frontier and the table cannot disagree - watched
failing by reintroducing each drifted copy in turn.

## What it changed

PPSA02664's honest record now reads **197 imports, 177 answered**. The table has an `Answered`
column, and a dash where a run predates the measurement.

## What this does not establish

**That the position between `imports` and `standing` is right**, only that it is below `imports`.
Whether answering ten functions is worth more than one percent of standing is a judgement nothing
here measures, and the guard says so.

**Nor that `standing` is redundant.** It answers a different question - how much of a *call count*
was real - which is the one D181 added it for, and a title that spins on an unimplemented function
still needs it to say so.
