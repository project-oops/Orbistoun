# D488 - The title-module base sits clear of the guest's own heap arena

**Status:** decided
**Date:** 2026-09-03

A title's own shipped modules are placed at an address chosen to avoid the address a
guest's own allocator may reserve its heap arena at, rather than at whichever high address a
loader would otherwise pick first.

**Why:** placing a title's modules at the same address a title's own allocator reserves for
its heap collides with that reservation, and a guest's fallback behavior after a failed
reservation is itself unstable across runs.

**Rejected:**
- Placing title modules at the first available high address with no regard for what a
  guest's own allocator might request there: works until a title reserves exactly that
  address, then fails unpredictably.
