# D431 - Guest mutexes and condition variables from the C runtime allow recursion by default

**Status:** assumed
**Date:** 2026-09-01

Every mutex the C++ runtime's threading layer constructs is created allowing same-thread
recursive locking, and a runtime sleep request is clamped to at most one second.

**Why:** The runtime's own type word distinguishing a recursive mutex from a plain one is not yet
measured; allowing recursion cannot itself raise a false deadlock before that measurement exists,
where forbidding it could. Clamping a sleep bounds the cost of a convention mix-up between an
absolute and a relative time value to a retry, not a multi-year hang.

**Rejected:** honouring an unmeasured recursion type bit - risks a spurious deadlock on a guess;
an unclamped sleep - risks an effectively permanent hang on a time-convention mismatch.
