# D104 - Blocked instructions name their dependency

**Status:** decided
**Date:** 2026-08-20

`model::BLOCKED` lists instructions whose semantics are understood but which wait on a missing
subsystem, each with its reason. It is not a to-do list: anything that could simply be written
is written.

**Why:** the worklist must tell an instruction nobody has looked at from one waiting on a whole
subsystem, because they rank differently.

**Rejected:**
- Mapping an export onto the observation buffer: an invented destination that appears to work.
- Leaving blocked instructions unlabelled: an afternoon and a subsystem look alike.
