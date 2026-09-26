# D039 - TOML for people, JSON for data, no database

**Status:** decided
**Date:** 2026-09-26

Anything a person edits - stub policy, settings, title files, knowledge - is TOML. Machine-
written data - the symbol database, traces, run reports - is JSON. Nothing uses a database.

**Why:** hand-editing the stub policy is the bisection loop, so readability is a requirement.
The machine-written files are large flat lists that tools read whole. Nothing here queries data
in a way a file does not serve.

**Rejected:**
- SQLite: an engine and a schema for data read whole.
- One format for everything: either hostile to editing or verbose for bulk.
