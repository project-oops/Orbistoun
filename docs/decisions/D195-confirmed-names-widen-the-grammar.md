# D195 - Confirmed names widen the grammar

**Status:** decided
**Date:** 2026-09-26

When a search confirms a name the grammar cannot yet spell, its words are written into a
`learned` list in `vendor.toml`, apart from the hand-written vocabulary. A word the grammar
already spells is refused, the written file must parse before it is kept, and a failure to
widen is never fatal.

**Why:** a confirmed name nobody can re-derive is an assertion again; its parts make it and its
neighbours derivable from the repository. A chosen word and a yielded word are different claims.
The learned list is used once per pattern, so it grows the search linearly rather than
squaring it.

**Rejected:**
- Printing suggestions for a person: a step that never happens unattended.
- Merging into the hand-written lists: loses provenance and squares the search.
