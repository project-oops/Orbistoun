# 591. Stuck and working no longer share an outcome

**2026-09-15** - the second half of the gap analysis's step 2, and the half that was already
measured but never recorded

## What was wrong

A run the clock ended was filed as `ran to the time limit`, whether the guest was working right up
to the last millisecond or had stopped asking for anything seventeen seconds earlier. That string
reads as *still going*, and for the title that gets furthest in this project it said the opposite
of what happened.

**The measurement already existed.** D645 built `Quiet` - silence, last activity, run length, and
the sample interval that bounds the claim - and `print_quiet` shows it in the run report. What it
never reached was the **outcome**: the string that goes into `compat/` and into the frontier
table, which is what a reader actually reads. So the report could tell stuck from working and the
record could not.

## What changed

`describe_end` now returns one of two stable strings for a run that reached the clock:

- `ran to the time limit` - the guest was still asking for things.
- `went quiet, then ran to the time limit` - `Quiet::is_notable()`, meaning no import or system
  call for at least a second **and** for at least half the run.

## Two things it deliberately does not do

**It does not embed the duration.** `compare` tests this string between runs to decide whether the
ending changed; a number in it would make every pair of runs differ and the field would stop
meaning anything. The quantity stays in `quiet`, where `Quiet::describe` reports it in the terms
it was measured, bounded by its own sample interval.

**It does not say "waiting for input".** The measurement is silence towards the host, and a guest
computing hard in its own code with no host calls reaches it too. Calling that *waiting* would
report more than was measured - the gap analysis's gap 5 is about titles gating on a button, but
this evidence cannot distinguish that from a long computation, and the string says only what it
knows.

## Made to fail

`a_run_that_went_quiet_is_not_filed_as_one_still_working` covers both directions, and the negative
half is the one that matters - a rule without it would file noise as a finding:

- no measurement at all → the old string, because nothing may be claimed about a silence nobody
  measured;
- 17s silent of 20s → the new string;
- 0.9s silent of a 1.5s run → the old string, under the one-second floor;
- 2s silent of 90s → the old string, a pause rather than a stop;
- a deliberate exit with 17s of silence → `the guest called exit`, because the guest's own
  decision outranks the clock.

Dropping the `is_notable()` check makes it fail on the floor case, with the message naming it.

## Validated on a real run, on the conservative side

`PPSA99980` is one of the three records carrying the flat string. Run now: 216 imports, 416,744
calls, `quiet` reporting *"no call in its last 0.0s of 20.0s"* - it was calling right up to the
clock - and the outcome correctly **stays** `ran to the time limit`.

That is the negative half confirmed against a live guest rather than a constructed trace, and it
also confirms the D645 machinery is populated on every clock-ended run.

## Negative result: nothing in the runnable corpus reaches the new string

Checked by running them rather than by reading records. `PPSA04263` faults, `PPSA25872` faults
(worklog 555), `obscene-payload` exits deliberately, `PPSA99980` works to the clock. The three
records carrying `ran to the time limit` belong to guests that either fault now or cannot be run
(below).

So the positive half is unit-tested and not currently observable. Worth knowing before anybody
reads the frontier and concludes the split does nothing: **it has nothing to fire on yet**, which
is a fact about the corpus and not about the rule.

**And D645's own example has moved.** That entry established the silence on `PPSA25872` -
310,987 calls and then nothing, identical at twenty seconds and at ninety. That title now faults
at `image+0x17554a3` instead, so the guest it was written about no longer ends the way it ended.

## Corpus hygiene, found while looking for a guest to test on

Two things, recorded rather than fixed - both change what gets measured, which is not a call to
make in passing:

- **`titles/obscene` is a dangling symlink.** It points at
  `<data>/titles/obscene`, which no longer exists. `ls titles/` lists it as though it were
  available and `compat/obscene.toml` holds two records for it. A run against it fails honestly
  (*"failed reading ... os error 3"*), so nothing silently lies - but the corpus advertises a
  guest it does not have.
- **`PPSA99980` is installed and not linked.** It sits in the data directory with a compat record
  and no entry under `titles/`, so the corpus does not include it and a sweep will not run it. It
  is the guest that validated this change, reached only by its installed path.

Whether the first should be removed or restored, and whether the second should be linked, are
questions about corpus membership - they change the frontier and what a sweep measures.
`orbistoun-cli corpus sync` exists and is the right place for the answer.
