# D018 - Guest call recording is allocation-free and attributed

**Status:** decided
**Date:** 2026-09-26

Every guest call is recorded on the dispatch path into a fixed-size atomic ring with a global
sequence number, the import index, its first argument and the guest return address. Recording
takes no lock and allocates nothing; the worker drains the ring into the run's call trace.

**Why:** guest call volume reaches tens of millions per run, and a recorder that blocks or
allocates changes the program it observes. The sequence number orders a multi-threaded run
after the fact, and the return address answers "which call site", which is usually the useful
question.

**Rejected:**
- Text logging per call: dominates the profile.
- A dynamically dispatched sink per call: a virtual call on every guest call.
- Counting by function only: loses order and call site.
