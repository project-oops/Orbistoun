# D316 - Decision numbers are reserved under a lock

**Status:** decided
**Date:** 2026-08-27

`./bin/orbistoun decide "<title>"` claims the next decision number under a `mkdir` lock and
writes a reservation before any body exists. The gate refuses duplicate numbers and
reservations left unfilled.

**Why:** several sessions write decisions at once, and "read the highest number and add one"
races over the minutes spent writing the body. Source cites decisions by number, so a
collision makes every citation ambiguous. `mkdir` is atomic on every platform this runs on. An
abandoned reservation reads like a recorded decision.

**Rejected:**
- Reading the highest number: races between sessions.
- Relying on the duplicate check alone: fires after citations already point at both.
