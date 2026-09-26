# D513 - Invented region bases and the address map

**Status:** decided
**Date:** 2026-09-26

Every fixed base orbistoun reserves is a named constant listed in `docs/ADDRESS_MAP.md` with its
owner, and a test fails when a base is missing from the map, stale in it, listed at the wrong
value, or within four gibibytes of another. Regions of orbistoun's own invention take bases in
the `0x0000_5E2x_0000_0000` family.

**Why:** bases spread across many crates are discoverable only by search, and a base chosen
without checking collided with the range the kernel hands out virtual reservations from, which
changed what the guest did. A base is a constant with a name and a value, so the map and the
source are two machine-readable sets a test can compare. The `0x5E2x` family is claimed by
nothing else.

**Rejected:**
- Choosing a base by reasoning about what is nearby: silently collided with an existing base.
- A prose-only map: drifts from the source with nothing to notice.
- Gating spans as well as starts: the map records starts, not lengths.
