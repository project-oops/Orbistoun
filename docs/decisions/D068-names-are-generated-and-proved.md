# D068 - Names are generated and proved

**Status:** decided
**Date:** 2026-09-26

A symbol name is proposed by this repository's grammar and word lists and accepted only when
its hash equals a NID a real module imports. No third-party name database is consulted,
bundled or downloaded. The vocabulary is data, so extending it never needs a rebuild.

**Why:** a NID is a truncated hash and cannot be inverted, so proposing and hashing is the only
route back. A match is proof and a miss proves only "not among those tried". A name produced by
a grammar in the repository and confirmed by arithmetic carries no provenance question.

**Rejected:**
- Loading a public NID database: somebody else's derivation, and it may come from firmware.
- Hand-typing confirmed names: an assertion nobody can re-derive.
