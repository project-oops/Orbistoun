# D071 - The hash suffix ships as a data file

**Status:** decided
**Date:** 2026-09-26

The NID hash suffix ships in selfish's `data/hash-suffix.toml`, embedded by `selfish-nid` at
build time and used by default. The file states what the value is, that it is `supplied` rather than
derived here, and how it verifies itself.

**Why:** the suffix is a salt on a name-mangling hash, one constant for the platform; it
decrypts and authenticates nothing, and requiring a user to supply it adds a step that protects
nothing. It cannot be derived from a name and hash pair, so it is recorded honestly as supplied.
Hashing published C library names with it matches real imports, and a wrong value matches none.

**Rejected:**
- A user-supplied suffix: a setup step with no benefit.
- A bare literal in source: an unexplained constant with no provenance beside it.
