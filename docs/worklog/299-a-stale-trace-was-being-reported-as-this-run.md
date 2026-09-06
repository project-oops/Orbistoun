# 2026-09-02 - (/loop) Every report today was a file from 01:50; the ctype work had moved the wall

Went looking for why the report named `_Getpctype` when the dispatcher said it was implemented. The
answer is not a labelling bug (D469's guess, now corrected): **`cmd_run` was reprinting an old trace
as though it were this run's** (D470).

## The chain

`cmd_run` reads the stored trace after the worker exits - "the trace it wrote" - without checking that
it wrote one. The worker for PPSA02664 dies before it can record anything, so the file read back was
the **01:50** one, from nine hours before `_Getpctype` was implemented at 10:13. It was then compared
against itself, so:

- `verdict same - nothing moved` (a file equals itself),
- `libc::_Getpctype ... nothing implements it` (true at 01:50),
- the same fault block every time - which I read as "deterministic, six runs in six". It was one file.

The A/B, one build, only the ctype registration changing: **without** ctype a fresh trace is written
and the guest faults at `0xb14be3` dereferencing `_Getpctype`'s placeholder; **with** ctype no trace is
written at all.

## Two corrections, both mine

1. **D469 is wrong** and now says so at the top. Its measurements stand - index 54 is bound, called 415
   times, never answers a placeholder - but the conclusion drawn from them (a mislabel) was built on
   the stale report. D443/D450/D459 were right all along: `_Getpctype` *is* that branch's wall.
2. **The ctype work did move the wall**, and I reported the opposite. With it the guest gets past
   `0xb14be3` and dies somewhere the worker cannot report. Progress, invisible, reported as `same`.

## The fix

`orbistoun_report::trace::Stamp` (modified time **and** length - either alone can repeat) plus a pure
`wrote_a_trace(before, after)`; `cmd_run` stamps the file beside `before` and afterwards either reports
or says `this run recorded no trace, so there is nothing of its own to report.` Pure decision plus thin
effectful wrapper, in `orbistoun-report` rather than the CLI so the GUI reaches the same verdict from
the same rule. Four tests, the first being **the guard made to fail** (unchanged stamp must read as no
trace); verified live both ways - it fires for PPSA02664 and stays silent for PPSA04263.

## Why all the instrumentation reported nothing (the general lesson)

Every probe here lives **inside the process that dies** and runs **after** the fault - collecting
registers, walking frames, reading guest memory to name the site, allocating, serialising JSON. Any of
those can fault again, and a second fault inside a fault handler ends the process rather than being
reported. Nothing was missing from the instrumentation; it was all downstream of the break.

What was missing was a check that a measurement **happened at all**. A hundred ways to describe a
trace, no way to notice there wasn't one - so absence borrowed the previous run's presence. Same shape
as the five tools principle 3 already lists, and the fix is two integers compared before believing a
file.

## State

clippy `--tests` clean for report/cli/libc/gen; **one pre-existing warning left**: `enter` in
`orbistoun-worker/src/lib.rs` is 113/100 lines, grown by an earlier session's spawned-thread TLS work
in this same uncommitted tree. Not this tick's, and not refactored blind - the guest-entry path is the
most delicate code here - but it will fail CI and should be split before anything is committed. fmt
clean; `~/.oops-identity/scan.sh` clean; nothing committed.

**Next**: why the worker dies unreportably for PPSA02664 with ctype installed. Candidates: a nested
fault inside the fault handler (it reads guest memory and allocates), a stack overflow (guard page
leaves no room to report), or a crash in orbistoun's own code where the guest-fault assumptions do not
hold. The tooling is honest now, so the next run's silence will be legible.
