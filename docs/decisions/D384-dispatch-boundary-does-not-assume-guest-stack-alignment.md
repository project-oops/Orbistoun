# D384 - The dispatch boundary does not assume the guest's stack alignment

**Status:** decided
**Date:** 2026-08-30

Code entered from a guest-held function pointer (a gadget, or any call site
this project does not control) establishes its own aligned stack frame and
restores the guest's original stack pointer before returning, instead of
assuming the caller's stack already met the calling convention's alignment.

**Why:** Every ordinary import is reached through a call site a compiler
wrote, which the ABI guarantees is aligned; a gadget is reached however the
guest's own code happens to call it, with no compiler-enforced guarantee.
Vectorized code that assumes alignment raises a fault on a stack that is off
by a few bytes, which nothing about the instruction set requires this
project's own code to assume.

**Rejected:**
- Trusting the guest to call with an aligned stack: true for a call site a
  compiler wrote, false for a raw gadget, and a boundary this project controls
  one side of must not assume the other side's convention.
