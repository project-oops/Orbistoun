# 411. The record could not see the work

**2026-09-04** - directed, while the Agc probe set is out for hardware

## What was done

Closed the gap worklog 409 and 410 both ended on. A day that took PPSA02664 from **35 unanswered
functions to 20**, and from **914 stubbed calls to 32**, moved no recorded number at all:
`standing` is an integer percentage of calls, and 0.22% and 0.008% both round to 100.

The fix was not more precision. **A percentage of calls belongs to whatever the guest loops on** -
this title spent 12,924 calls in one wait - so it swings on where a title spins while the thing a
person acts on barely moves. What moved is *distinct imports called with nothing behind them*: it
counts functions, is stable against a hot loop, and is literally the work list.

`Status::unanswered` now records it, `answered()` ranks on it, and the table has an **Answered**
column. PPSA02664's honest record reads **197 imports, 177 answered**.

## The bug found on the way, which mattered more

**Three copies of the ranking existed and two had already drifted.** `beats` decides what gets
recorded, `frontier` decides what a shim shows, `render_markdown` decides the table - and neither
of the last two gained `frames` when D558 added it **this morning**. The rung I added today ranked
one way in the record and another in the table.

`frontier`'s own doc had already warned about it, in as many words: *a table that disagreed with
the thing deciding what to record would be the more convincing of the two and the wrong one.* It
was right. There is one `ranking_key` now, and a guard that the three cannot disagree - watched
failing by putting each drifted copy back.

That is the second time today a warning written into this codebase turned out to be describing
something that had already happened (D523's per-thread record was the first).

## Where it ranks

Below `imports`, which is D182's shape a third time: a guest that gets further calls **more**
imports, and some of what it newly calls will be unimplemented - so a run can reach further and
have more unanswered. Ranked above imports, going further would report as going backwards.

## Optional, and why that matters

`None` is *not measured*, not *nothing unanswered*. Collapsing them would let a stale record claim
a perfect score and refuse every honest run after it - the exact trap `propping` sprang this
morning, when 33 records deserialised as honest. An unmeasured record ranks as though nothing was
answered, which is not a claim about the run but about the record, and it self-heals on that
title's next run.

## Guards

Nine, each watched failing: `answered` dropped from the ranking; `answered` ranked above imports;
unmeasured reading as nothing-unanswered; the count taken from calls instead of functions; the
count taking every import rather than the unanswered ones; the value never reaching the record;
and the table and the frontier each keeping their drifted copy.

## Surprise

**Every record in the table now shows a dash.** That is correct - none of them measured this - and
it is also a plain statement of how much of the compatibility record is older than today. It will
fill in one title at a time as each is re-run, which is the honest way for it to fill in.
