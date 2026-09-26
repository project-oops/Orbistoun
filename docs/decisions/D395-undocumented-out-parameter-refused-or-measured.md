# D395 - An undocumented out-parameter is refused, or measured, never invented

**Status:** decided
**Date:** 2026-08-30

A call filling a structure whose field layout no lawful source describes
answers a negative status by default; under a diagnostic setting it instead
fills the structure with markers that name their own offset, so a guest's own
reads reveal which fields it actually wants.

**Why:** Inventing a plausible layout produces a run that looks like it worked
while feeding a guest fields that mean nothing; refusing outright is honest
but stops short of measuring, when the guest itself is the source that can
settle it. A negative status is also the right shape for a failure - a small
positive placeholder reads to a status-checking caller as success.

**Rejected:**
- Filling the structure with an invented plausible layout: not sourced from
  anything, and a guest that reads it acts on a fabrication.
- Refusing without ever measuring: leaves the real layout permanently unknown
  when the guest could reveal it a field at a time.
