# D443 - Orbistoun's own fixed address regions stay clear of addresses a guest's own allocator computes

**Status:** decided
**Date:** 2026-09-01

A region this project reserves at a fixed address for its own bookkeeping is chosen to be clear
of every address a guest's own allocator is observed to request as a reservation hint, not merely
an address that looks unused.

**Why:** A guest's allocator sizes its own arena relative to the hint address it passed to
reserve a range; if this project's own fixed region already holds that address, the guest's
reservation is displaced to an unrelated address and the guest's own size arithmetic underflows.
"Clear of this project's own regions" is not the same claim as "clear."

**Rejected:** picking a fixed region's address by inspection alone, without checking it against a
guest's observed reservation hints - the exact mistake this decision corrects.
