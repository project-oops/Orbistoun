# D652 - Signals delivered into owned waits

**Status:** decided
**Date:** 2026-09-09

A signal raised on another thread is delivered only when the target is parked in a wait orbistoun
owns: the wait checks a per-thread pending slot, runs the handler on that thread with the lock
dropped, and goes back to waiting. A raise on a thread that is not parked answers the loud
placeholder, never success.

**Why:** guest code runs natively, so a thread executing guest code cannot be interrupted; a
thread asleep in orbistoun's own wait is at a point orbistoun controls. Answering success for a
signal that cannot be delivered tells a collector a handler ran, and it then waits forever or acts
on state nobody produced. The slot is a cached `Arc` read without a lock, because reading the
thread table inside the wait predicate would invert lock order against the raising call.

**Rejected:**
- Checking the slot at thunk dispatch: a target that never calls again never takes the signal.
- Answering success when delivery is impossible: a dropped signal is invisible from inside the guest.
- Returning from the wait as woken: hands the guest a satisfied wait whose condition is still false.
