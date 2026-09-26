# D470 - A run that wrote no trace reports nothing

**Status:** decided
**Date:** 2026-09-02

A run's report is built only from a trace that run itself wrote. If the worker exited
without writing one, the run reports that it recorded nothing, rather than reprinting a
trace stored by an earlier run.

**Why:** comparing a run's outcome against itself is not a measurement. A worker that dies
before writing a trace previously produced a report that looked like a real, unchanged
verdict when nothing had actually been observed that run.

**Rejected:**
- Trusting that the trace file is always fresh because the worker has just exited: this is
  the assumption that let a stale trace be reported as current.
