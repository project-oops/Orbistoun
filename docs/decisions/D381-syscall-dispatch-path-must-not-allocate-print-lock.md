# D381 - The syscall dispatch path must not allocate, print, or lock

**Status:** decided
**Date:** 2026-08-29

Code reached from a guest's syscall gadget runs on the guest's own stack and
records what happened into lock-free, allocation-free state; only a separate
reporting layer, reached on a frame this project controls, formats and prints
it.

**Why:** Every frame below the gadget executes on a stack this project did not
size and does not own, so an allocation, a lock, or a formatting call there can
fault before the record is even made. Splitting recording from reporting keeps
the invariant in one place instead of relying on each dispatch path to
remember it.

**Rejected:**
- A locked container for call records, as the ordinary import path uses:
  correct there because that path runs on a frame this project arranged, wrong
  on the dispatch path because it does not.
- Printing directly from the dispatcher: formatting allocates and can fault on
  the guest's stack before anything is recorded.
