# D470 - A run that wrote no trace reports nothing, rather than reprinting an old one

**measured** - 2026-09-02 (user-directed /loop; corrects [D469](D469-the-getpctype-wall-was-misattributed.md))

## What actually happened

`cmd_run` reads the stored trace *after* the worker exits, on the reasoning that the worker has just
written it:

```rust
// After the worker has exited, so the trace it wrote is complete.
if let Some(after) = previous_trace(path) {
```

**"the trace it wrote" was an assumption, not a check.** When the worker dies without writing one, the
file read back is the *previous* run's. It is then compared against itself - `before` and `after` are
the same bytes - so the verdict reads `same - nothing moved`, the fault block shows the old fault, and
the findings list the old unimplemented functions. Every line is presented as this run's measurement
and none of it is.

For PPSA02664 the stored trace was from **01:50**; `_Getpctype` was implemented at **10:13**. Every run
after that read the 01:50 file. That is why six runs gave byte-identical reports and read as
"deterministic": they were one file.

The A/B that settled it, on one build, changing only whether the ctype implementations are registered:

| | trace written? | what the report showed |
|---|---|---|
| without `ctype` | **yes**, fresh | guest faults at `0xb14be3` dereferencing `_Getpctype`'s placeholder |
| with `ctype` | **no** | the 01:50 file, unchanged |

## So the original diagnosis was right

[D443](D443-ppsa02664-s-allocator-wall-was.md) / [D450](D450-ppsa02664-s-two-walls-are-a-thread.md) /
[D459](D459-trace-records-what-a-call-answered.md) were correct: `_Getpctype` **is** the wall in that
branch, and the guest does dereference its placeholder. D469's conclusion - that the report mislabels
the faulting call - is **wrong**, and D469 now says so at the top. Its individual measurements stand
(index 54 is bound, is called 415 times, and never answers a placeholder); what was wrong was reading
a stale report as evidence about the current build.

**And the ctype work moved the wall.** With it, the guest gets past `0xb14be3` and dies somewhere the
worker cannot report. That is progress that was invisible, reported as `same`.

## The change

`orbistoun_report::trace::Stamp` (modified time *and* length, because either alone can repeat) and a
pure `wrote_a_trace(before, after)`. `cmd_run` takes a stamp beside `before`, and afterwards either
reports or says plainly:

```
this run recorded no trace, so there is nothing of its own to report.
```

The pure/effectful split is the shape `orbistoun-mem` established, so the decision is testable without
a filesystem, and it lives in `orbistoun-report` rather than the CLI so the GUI reaches the same
verdict from the same rule (principle 13, D160). Four tests, and **the first is the guard made to
fail**: an unchanged stamp must read as "no trace from this run". Verified both ways on real runs -
it fires for PPSA02664 and stays silent for PPSA04263, which does write one.

## Why so much instrumentation reported nothing

The honest answer, because it is the general lesson. Every probe this project has is **inside the
process that dies** and runs **after** the fault: the handler collects registers, walks frames, reads
guest memory to name the fault site, allocates, and serialises JSON. Any of those can fault again -
and a second fault inside a fault handler is not reported, it is the end of the process. Nothing was
missing from the instrumentation; the instrumentation was downstream of the thing that broke.

What was missing was a **check that a measurement happened at all**. The run had a hundred ways to
describe a trace and no way to notice it had not got one - so absence borrowed the previous run's
presence. That is the same failure as the five tools in principle 3's list, and the cheapest possible
fix: compare two numbers before believing the file.

## Still open

The worker dies unreportably for PPSA02664 with ctype installed, and *why* is not diagnosed here. The
candidates are a nested fault inside the fault handler, a stack overflow (whose guard page leaves no
room to report), or a crash inside orbistoun's own code where the guest-fault path's assumptions do not
hold. The next run should find out, and it now has honest tooling to do it with.
