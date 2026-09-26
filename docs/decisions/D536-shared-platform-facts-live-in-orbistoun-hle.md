# D536 - Shared platform facts live in `orbistoun-hle`

**Status:** decided
**Date:** 2026-09-04

When two sibling subsystem crates need one platform fact, such as the clock identifiers and the
single monotonic origin behind `clock_gettime` and `sceKernelClockGettime`, the fact lives in
`orbistoun-hle`, which both already depend on.

**Why:** the facts belong to the platform rather than to either library, and siblings must not
depend on each other. A copy in each crate compiles and drifts: two monotonic origins give a guest
two different elapsed times, and each crate's own tests stay green.

**Rejected:**
- One sibling depending on the other: reaches sideways across the dependency spine.
- Copying the code into both crates: two origins that disagree with nothing to notice.
