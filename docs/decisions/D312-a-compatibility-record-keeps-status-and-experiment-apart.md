# D312 - A compatibility record keeps status and experiment apart

**Status:** decided
**Date:** 2026-08-27

A compatibility record holds two slots: `[status]` for what the emulator does unassisted and
`[experiment]` for the furthest a run got under any stub answer given by name or by default. A
run is routed to its slot, compared only within it, and never refused on policy grounds;
`--force` only overwrites a better entry in the same slot.

**Why:** a run helped by stub answers reaches further by construction and must not become a
best-ever no honest run can beat. Refusing it discarded a real number and made every measured
policy need `--force`. Comparability is whether a run was helped or not; counting overrides
would recreate the refusal.

**Rejected:**
- Refusing helped runs: loses the result and stalls the loop.
- One best-ever entry: mixes the helped number with the honest one.
- Checking only the global default answer: per-function answers pass through unnoticed.
