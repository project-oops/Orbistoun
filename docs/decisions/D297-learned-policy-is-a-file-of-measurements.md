# D297 - Learned policy is a file of measurements

**Status:** decided
**Date:** 2026-09-26

`learned.toml` holds `[[measurement]]` entries - function, the title it was measured on,
date, tool version, how it is known, evidence, answer, writes and `assumes` - and the policy
a run uses is derived from them. `--verify` re-derives an entry in a fresh data directory
with no learned file.

**Why:** a measurement from a binary the submitter owns is reproducible and falsifiable by a
command, so it can be received from anyone without a provenance question. The title field
stops a value measured on one guest reading as a platform fact. A machine that has applied a
measurement no longer hits the wall it came from, so verification must run from the state the
measurement was taken in. An entry nobody can verify locally is accepted as `assumed`.

**Rejected:**
- A settings cache: cannot be sent or checked.
- Verifying against the accumulated state: the applied answer hides the wall and confirms itself.
