# D582 - The guest clock is logical by default

**Status:** decided
**Date:** 2026-09-08

Every clock a guest reads comes from `clocks::since_start_nanos`, which by default advances one
microsecond per reading and by the requested duration on a sleep; the wall clock is a fixed
synthetic epoch plus that elapsed time. `ORBISTOUN_CLOCK=host` uses real time instead.

**Why:** a clock read from the host makes two runs of one build diverge, and a measurement that
cannot be repeated is not one. A clock that repeats need not stand still: a fixed step per
reading keeps timing plausible, ends a millisecond spin in a thousand readings, and lets a guest
that waits for time to pass proceed. One source keeps process time, the tick counter and the
monotonic clock consistent and switched by one setting.

**Rejected:**
- Host time by default: runs stop being comparable.
- A pinned clock: stops any guest that waits for time to pass.
- A clock per library: conversions between them drift, and one setting cannot govern them all.
