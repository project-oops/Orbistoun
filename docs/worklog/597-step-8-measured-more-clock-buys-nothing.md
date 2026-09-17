# 597. Step 8 measured: more clock buys nothing, and input is one hardware run away

**2026-09-15** - the gap analysis asked for "input routes and minute-scale runs". Both halves are
now accounted for, and neither is the work it looked like.

## Minute-scale runs buy nothing, measured

`PPSA99980` is the only guest in the corpus still calling when the clock ends - every other title
faults or exits, and this one was calling right up to the last sample (worklog 591). So it is the
one title where more time could possibly buy more.

| limit | imports | calls | verdict |
|---|--:|--:|---|
| 20s | 216 | 416,744 | - |
| **120s** | **216 (+0)** | 459,254 (+42,510) | `same - nothing moved` |

**Six times the wall clock, zero new imports.** The guest is not idle - it makes forty-two
thousand more calls - it is looping over ground it has already covered.

That is the third independent time this has been measured and the third negative: D645 found
`PPSA25872` identical at twenty seconds and at ninety, and worklog 555 found Terminator identical
at two seconds and at twenty. Three titles, four limits, nothing moves.

**So the limit is not what is holding any guest back**, and "runs measured in minutes rather than
seconds" is a change with no measurable effect. `--limit` already accepts any value; nothing needed
building, and nothing should be.

## Input is blocked, precisely, and the project already knew where

The gap analysis says `orbistoun-input` "answers an all-zero pad, and the two functions that would
write a real pad structure are declared and not implemented". **That is out of date** - it is
reading a module doc that is out of date (below).

What is actually true: obSCEne measured the structure on hardware - 120 bytes, all 120 written,
full contents at rest (`100-input/read-extent`, sweep 20260909-110725) - and `scePadReadState` is
implemented and writes the whole extent every call.

What it cannot do is report a *press*. `pad::AT_REST` says so in as many words: which offset
inside those bytes carries the buttons is an **inference** from one at-rest image, so mapping a
live pad state onto them would publish that inference as a measurement. The transport exists
(`latest` holds what the window sent, with the shell's own button already stripped) and stays
deliberately unread.

**It is one hardware run from working**, and the request is already filed:
`REQ-20260910T0650Z-d1c4`, still OPEN, asks for `100-input/button-bits` and
`100-input/stick-trigger-range` with a controller actually used.

So a file of timed pad states would have fed a function that cannot express a press. Not built.

## Two more stale documents, the fifth and sixth

Both said the pad structure was unmeasured and the read functions unimplemented. Both stopped
being true on 2026-09-09, and both are what the gap analysis read:

- `orbistoun-input`'s module doc - now records the measurement, that the functions are
  implemented, and that what remains is the *offsets* rather than the structure.
- `latest.rs` - its reasoning was right and its premise was not. The transport stays unconsumed
  for the original reason in a sharper form: the encoding is a measurement, and the part still
  unmeasured is narrower than "the structure".

Six stale documents in two days is a pattern rather than a run of bad luck. Every one of them
understated what the code does, and every one was believed by a document written later.

## What was built instead: the budget and the clock stop reading alike

Chasing the limits turned up a live defect. **D238 already requires this** - *"'ran out of clock'
and 'made the calls it was allowed' call for different next steps and must not read alike"* - and
it was not being honoured in the outcome. The worker exits with `TIME_LIMIT_EXIT` or
`CALL_BUDGET_EXIT`, but an exit code is read by the *parent* and the trace is written before it,
so both endings arrived at `describe_end` as the same absence and both recorded
`ran to the time limit` - including for a guest that never ran out of clock at all.

`CallTrace::ended_by` is now set by the branch that stops the run, and a spent budget records
`spent its call budget`.

**Not inferred from `total_calls >= call_budget`**, which was the tempting shortcut and is exactly
the failure principle 3 names - a report naming a cause it did not determine. The test proves the
difference: point it at the call count instead of the recorded reason and a *clock*-ended run that
happened to make twenty million calls is misreported as a budget, which is the case that would
have gone unnoticed.

Four more cases pinned: a budget-ended run that also went quiet is still a budget (it cannot have
been waiting if it was spending calls fast enough to run out); a deliberate exit outranks either
limit; a fault outranks everything.

## Gate state

fmt clean, clippy `--workspace --all-targets -D warnings` clean, `cargo test --workspace` 2,339
pass / 0 fail, worklogs unique, identity scan clean.
