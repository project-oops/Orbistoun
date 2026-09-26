# D571 - The call ring is circular, with a separate opening record

**Status:** decided
**Date:** 2026-09-04

The recorded-call ring wraps, so `CallTrace::tail` holds the calls before the fault; the opening
of the run is kept in its own record that is never overwritten. Each slot stores its call's
sequence number, readers order by sequence, and a return is written only if the slot still
belongs to that call.

**Why:** the tail exists for the neighbourhood of the wall, and a ring that fills once and stops
shows calls from early boot for any long run. The opening has its own reader, which a small
fixed record serves, so neither question is traded for the other. After a wrap slot order says
nothing about the guest, and writing a return into a slot a later call owns attributes it to the
wrong function.

**Rejected:**
- Keeping the first calls and calling them the tail: the report claims more than it measured.
- One ring for both ends: the opening and the fault compete for the same slots.
