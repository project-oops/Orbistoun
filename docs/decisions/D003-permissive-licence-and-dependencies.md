# D003 - Permissive licence and permissive dependencies

**Status:** decided
**Date:** 2026-09-26

orbistoun is MIT OR Apache-2.0, and `deny.toml` admits only permissive dependency licences plus
MPL-2.0. A data or font licence on a crate that ships data rather than code is a scoped
per-crate exception with its reason written beside it.

**Why:** preventing closed forks is not a goal, and a GPL dependency would relicense the
binary. MPL-2.0 is file-level copyleft: it covers modifications to its own files, never this
code. A data licence argument does not transfer to a code dependency under the same
identifier, so it is granted per crate rather than globally.

**Rejected:**
- GPL or AGPL: binary-level copyleft over the whole emulator.
- Dropping MPL-2.0 crates for hand-written replacements: worse code for a concern that does not apply.
- A blanket allowance for data licences: it would admit code under the same identifier.
