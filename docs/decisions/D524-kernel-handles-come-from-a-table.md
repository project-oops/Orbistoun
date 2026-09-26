# D524 - Kernel handles come from a table

**Status:** decided
**Date:** 2026-09-26

A kernel object a guest creates, such as an event queue, gets a handle from a table that records
it, the handle is written through the out-parameter, and a later call naming a handle the table
does not hold is refused with the measured vendor `ESRCH`. The table carries the guest's own name
for the object so a run report can list it.

**Why:** an unwritten out-parameter leaves the guest holding whatever was there, and every later
call is handed a value orbistoun never issued. A check that refuses unknown handles is only
trustworthy when the run shows the round trip succeeding, which the reported table does.

**Rejected:**
- Answering `Ok` without issuing a handle: the guest passes garbage to every later call.
- Accepting any handle: a guest passing a value nothing issued is told it worked.
- Building full event delivery before a guest waits: fixes a shape no caller has shown.
