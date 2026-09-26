# D156 - Nothing reachable from a guest call may panic

**Status:** decided
**Date:** 2026-08-20

Code reachable from a guest call never panics: arithmetic on guest values is checked, and a
hostile value - `u64::MAX` as a length, an address hint, an alignment - is refused.

**Why:** a guest-facing implementation runs on a frame entered through an `extern "sysv64"`
boundary, and unwinding across it is undefined behaviour that presents as an unattributable host
fault. A guest is entitled to pass any value at all.

**Rejected:**
- Panicking on overflow as ordinary Rust does: an unattributable crash instead of an answer.
- Catching unwinds at the boundary: still undefined across the foreign frame.
