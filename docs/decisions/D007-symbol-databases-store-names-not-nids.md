# D007 - Symbol databases store names, not NIDs

**Status:** decided
**Date:** 2026-08-19

A symbol database holds names only; every NID is derived from its name when the database loads.

**Why:** a file holding both could contain a pair that does not hash to each other, and the
inconsistency would surface much later as an unexplained unresolved import. Derivation makes
the file unable to disagree with itself.

**Rejected:**
- Storing name and NID pairs: two copies of one fact, free to diverge.
