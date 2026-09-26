# D300 - A stub region states how its base arrives

**Status:** decided
**Date:** 2026-09-26

A policy entry may give the guest one region, `StubRegion { via, bytes }`, delivered either
through an argument slot or as the return value. The service reserves the region before the
guest starts and installs a concrete base per symbol; the thunk does a single store or answer.
`bytes` is an assumed value in the policy file.

**Why:** a region through an out-parameter and a region as an allocator's answer are one
concept, and expressing both lets the loop try each against answering zero. Reserving at call
time would allocate on the guest's stack inside the trampoline. Nothing measured says how much
space a guest wants, so the size stays visible and changeable without a rebuild.

**Rejected:**
- Separate write and return concepts: only one of them could hand back a region.
- Deriving the size from an argument: bakes an assumption into code.
- Arbitrary side effects in policy: a program in a data file.
