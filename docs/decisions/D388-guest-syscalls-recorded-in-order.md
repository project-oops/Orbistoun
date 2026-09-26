# D388 - Guest syscalls are recorded in order, not as a presence bitmap

**Status:** decided
**Date:** 2026-08-30

The first sixty-four syscalls a guest makes are recorded in call order, each
with its first argument, into a lock-free fixed array; the report states both
how many were kept and how many were made in total.

**Why:** A bitmap can say which numbers a guest asked for but never when, and
whether a call happens before or after a guest gives up is often the entire
question. Recording order needs no allocation and no lock, so it can run on
the dispatch path (D381), and stating the total alongside what was kept keeps
a truncated list from reading as a complete one.

**Rejected:**
- A presence bitmap: answers "which" and not "in what order", which is the
  question that actually needed answering.
