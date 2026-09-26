# D005 - Interception is linking, not hooking

**Status:** decided
**Date:** 2026-08-19

Guest imports are resolved by NID against the registry, and the loader writes each resolved
address into the guest's relocation slot. There is no instrumentation pass.

**Why:** writing an address into a linkage slot is the interception, so the guest calls
whatever the slot holds. The complete list of what a title needs is available statically,
before a guest instruction executes.

**Rejected:**
- Patching guest code at call sites: rewrites the program being measured.
- Trapping calls at run time: costs a fault per call and hides the static import list.
