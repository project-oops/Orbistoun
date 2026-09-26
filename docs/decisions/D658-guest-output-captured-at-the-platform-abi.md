# D658 - Guest output captured at the platform ABI

**Status:** decided
**Date:** 2026-09-09

What a guest formats or writes through the C library's format family, the debug-output call and
the standard descriptors is captured into a fixed 64 KiB ring of the most recent bytes and shown in
the report unclassified, in the guest's own order.

**Why:** a failing guest usually says why, and its own words need no interpretation. The platform
ABI is the one surface every guest shares, so the capture works for any engine or payload without
knowing about it. A bounded ring keeps recording allocation-free and keeps a guest printing in a
loop from changing the run; the most recent bytes are kept because a guest describes its problem
just before it stops.

**Rejected:**
- An engine-specific log reader: rewritten per engine and useless on a payload.
- Ranking lines by words like "error": a heuristic presented as a diagnosis.
- An unbounded log: changes the program it observes.
