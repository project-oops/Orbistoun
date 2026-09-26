# D279 - The provenance size guard fails binary files only

**Status:** decided
**Date:** 2026-08-25

A file over a megabyte outside `assets/` fails the provenance guard when it is binary; a large
text file is listed on every run and does not fail.

**Why:** a firmware dump, decrypted title or disguised container is binary, and size was only
a proxy for that. The symbol database is large UTF-8 text and is the audit trail provenance
requires. Text derived from the hardware or vendor material is caught by the symbol audit, which re-derives every name from
this repository's own inputs; listing large text keeps the threshold visible.

**Rejected:**
- Failing every large file: fails on the evidence the guard protects.
- Dropping the size check: loses the one guard against a dump with a disguised extension.
