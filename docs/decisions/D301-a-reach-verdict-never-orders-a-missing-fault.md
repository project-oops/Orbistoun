# D301 - A reach verdict never orders a missing fault

**Status:** decided
**Date:** 2026-09-26

Comparing two runs, a run that did not fault is not ordered against one that did; the count
of distinct imports reached decides alone. A run that ended without a fault is flagged in the
verdict (`Progress::ended_without_a_fault`) as having stopped measuring progress.

**Why:** a missing fault is the absence of a position, and ordering it reports a run that
stopped faulting as `BACK` and one that started faulting as `FURTHER`. Once no wall remains,
reach saturates: two answers that both clear the last wall are indistinguishable to it, and
only a conformance check can separate them.

**Rejected:**
- Treating no fault as further or behind: a fabricated ordering on the word the loop steers by.
- Reporting `FURTHER` from a saturated run without comment: reads as confirmation.
