# D475 - A prefixed export delegates to its unprefixed twin

**Status:** assumed
**Date:** 2026-09-02

A POSIX-namespaced export with an already-implemented unprefixed equivalent delegates to
whatever implements that equivalent, resolved through the same table the unprefixed name
resolves through, rather than pointed at the name directly.

**Why:** the two names share a POSIX signature, so implementing the pair separately would
duplicate a claim already carried elsewhere in the project's knowledge. Resolving through the
table rather than the name avoids landing on an alias that implements nothing itself.

**Rejected:**
- Delegating directly to the unprefixed name: several unprefixed names are themselves
  aliases, so this points at nothing.
