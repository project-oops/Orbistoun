# D649 - Unconfirmable names are knowledge, not symbols

**Status:** decided
**Date:** 2026-09-09

A name attributed to a hash by published tables but which does not hash to that value is recorded
as a knowledge entry at `known_by: published`, stating that it cannot be confirmed. It never
enters the symbol database or the name-search vocabulary.

**Why:** every name in the symbol database is confirmed by its hash, so nothing there can be wrong
in a way that produces a false name. Admitting one unconfirmable name would replace that
structural guarantee with a policy. Some libraries carry alias identifiers that no name reproduces,
and a reader meeting the bare hash still needs to know what it is believed to be.

**Rejected:**
- Adding the name to the symbol database: breaks the hash-confirmed guarantee.
- Adding it to the vocabulary: a word that does not hash to the target contributes nothing to a search.
