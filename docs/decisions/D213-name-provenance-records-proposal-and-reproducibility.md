# D213 - Name provenance records proposal and reproducibility

**Status:** decided
**Date:** 2026-09-26

Every name in the symbol database records how it was proposed, written by the code that found
it on the day of discovery and never rewritten: `published-standard`, `generated` (a pattern and
an index), `static` (module strings), `runtime` (call trace, argument dump, probe transcript) or
`supplied`. Evidence and reproducibility tiers are derived from the variant, never stored. The
audit re-derives every claim, and `supplied` never verifies. Databases accumulate.

**Why:** the hash confirms every name the same way, so provenance describes the proposal, not the
proof. A claim that is re-run rather than read makes a forged record fail as loudly as a missing
one. A string at rest and a conclusion from a run are different claims with different checks.

**Rejected:**
- One `observed` bucket: understates what can be checked.
- Stored tiers: a record could claim a tier its method does not support.
- Overwriting on each sweep: turns a first derivation into a timestamp.
