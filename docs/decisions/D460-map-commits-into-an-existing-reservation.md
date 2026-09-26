# D460 - Mapping direct memory into a range a guest already reserved commits into it, it does not reserve again

**Status:** decided
**Date:** 2026-09-01

Placing direct memory at an address the guest already carved out with its own reservation call
commits into that existing reservation; only an address the guest did not pre-reserve triggers a
fresh reservation.

**Why:** A guest's normal sequence is to reserve a virtual range and then place physical memory
inside it at that same address. Treating the second call as a second reservation of the same
range reports a conflict, which the guest reads as being out of memory, when the range was never
available for a second claim in the first place.

**Rejected:** unconditionally reserving on every map call - the correct behaviour only for a range
the guest did not already reserve, and wrong for the common reserve-then-map sequence.
