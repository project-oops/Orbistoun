# D408 - Unmodeled firmware handoff fields are zero, not markers

**Status:** decided
**Date:** 2026-08-31

When a run presents a firmware, handoff structure fields this project does
not model are set to zero rather than a marker, and a field known to be a
pointer is given a real pointer of the right shape, backed by mapped firmware
memory, never a fabricated address in the platform's own unreachable address
range.

**Why:** A guest checking a field against null branches wrongly if it finds a
marker there instead; matching zero for an unmodeled field keeps that check
honest. Certain fields hold addresses in a range this project's host process
cannot map at all, so those are answered with a real, dereferenceable
stand-in of the same shape - non-null, and backed by memory a guest can
actually read - rather than left as a marker or an address nothing can back.

**Rejected:**
- Leaving unmodeled fields as markers: a guest that checks a field for null
  branches incorrectly.
- Fabricating the platform's own unreachable addresses directly: they fall
  outside what a host process can map at all.
