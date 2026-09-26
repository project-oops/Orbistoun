# D406 - A firmware memory model is a separate crate from the named interface

**Status:** assumed
**Date:** 2026-08-30

The address space underneath the named kernel interface - reached only by a
guest that bypasses that interface and computes raw addresses - is modeled in
its own crate, as a region of this project's own zeroed, mapped memory at an
address of this project's choosing, stood up only when a run presents a
firmware.

**Why:** The named kernel interface answers "what does this call do"; the
memory underneath it answers "what is at this address", and those are
different questions with different provenance. Keeping them in separate
crates keeps a guest that reaches past the interface from blurring into the
interface itself, and a run presenting no firmware pays nothing for the
region existing.

**Rejected:**
- Folding the firmware region into the named kernel interface's own crate:
  mixes a clean call-level emulation with raw address-space modeling that
  only a minority of guests ever touch.
