# D463 - The guest mapping arena sits clear of the thunk data blocks

**Status:** decided
**Date:** 2026-09-01

A guest map placed with no address hint starts from a base kept a full terabyte clear of
the thunk's own reserved data blocks, so no address can belong to both arenas.

**Why:** a guest map with no hint and the thunk's reserved data began at the same address,
so the first such map failed to reserve and the guest read that failure as out-of-memory.
Separating the two lets an address alone identify which arena it belongs to.

**Rejected:**
- Leaving both arenas at their original bases: they occupied the same address and collided
  the first time a guest exercised the path.
