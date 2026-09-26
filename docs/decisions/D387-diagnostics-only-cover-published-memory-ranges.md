# D387 - A diagnostic instrument only covers memory a run has published

**Status:** decided
**Date:** 2026-08-30

A guest thread publishes its own stack span, into a fixed, lock-free registry,
the moment the thread exists; every instrument that reads guest memory checks
a request against the ranges a run has published and reports what it cannot
see as unknown rather than as a wild pointer.

**Why:** The set of readable ranges is fixed once before a guest starts, but a
guest thread's stack does not exist at that moment, so every instrument
reading one reported a false "wild pointer" verdict about its own blind spot.
Publishing a thread's span as soon as it has one, without allocating or
locking, keeps that registry usable from the same restricted context the
dispatch path runs in (D381).

**Rejected:**
- Publishing every readable range once at guest entry: correct for the image
  and the main stack, silent about every thread stack created afterward.
