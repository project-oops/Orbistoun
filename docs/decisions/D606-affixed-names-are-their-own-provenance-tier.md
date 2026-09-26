# D606 - Affixed names are their own provenance tier

**Status:** assumed
**Date:** 2026-09-08

The name sweep derives candidates from names already held, by the prefix, suffix and
substitution rules in `crates/orbistoun-names/data/affixes.toml`, and a proved one is recorded as
`Method::Affixed { seed, rule }`. A seed is only a name already held or proved, the derivation runs
one pass, and a seed equal to its derived name is refused.

**Why:** aliases in an export table are a finished name with something added or swapped, which no
compositional grammar reaches. The seed list changes with every successful search, so an index
into a product of seeds goes stale for reasons unrelated to the record; one rule applied to one
string is rechecked directly and reads in a diff. An unproved seed would let a proved name rest on
a guess, and iterating multiplies cost to buy names nothing suggests exist.

**Rejected:**
- Expressing the rules as a `Pattern`: every affixed record goes stale when the seed list grows.
- Iterating to a fixed point: cost grows with depth for unsupported names.
- Candidates as seeds: an audit cannot catch a proof built on an unproved name.
