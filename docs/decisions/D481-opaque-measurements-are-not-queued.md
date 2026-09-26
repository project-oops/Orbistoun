# D481 - A measured value that is not a property of the interface is opaque, not queued

**Status:** decided
**Date:** 2026-09-02

A constant measurement whose value is a fact about the specific machine or run rather than
about the interface itself is recorded in its own list, separate from the measurements this
project asserts and the ones still outstanding.

**Why:** a measurement that will never become a claim or a test does not belong queued as
future work. Leaving it in the outstanding list stops that list from being read as a queue of
completable items.

**Rejected:**
- Leaving such a measurement in the outstanding list: it never resolves into a test, so a
  queue that contains it is no longer a queue of completable work.
- Asserting it as a claim: it is a fact about the machine or run that produced it, not a
  property the interface guarantees.
