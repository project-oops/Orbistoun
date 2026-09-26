# D188 - The shipped symbol database loads by default

**Status:** decided
**Date:** 2026-08-21

`symbols/generated.json` is embedded from the workspace root and loaded unless a path is
supplied, and the default lives where naming happens, not in a shim.

**Why:** a run that ignores the committed database reports names the tree already has as unknown
and recommends searching for them. One file serves as both the audited and the loaded database,
so they cannot diverge.

**Rejected:**
- Opt-in loading: findings recommend work already done.
- A copy inside the crate: two databases that can diverge.
