# D305 - A plain symbol name is hashed into a NID

**Status:** decided
**Date:** 2026-08-26

A dynamic symbol whose name is not NID-encoded is hashed with the registry's hasher, so every
`RawImport` carries a NID and resolves one way. Library and module attribution is a `NameForm`
whose accessors return `Option`. The symbol count is read from `DT_HASH` when present and
walked out of `DT_GNU_HASH` otherwise.

**Why:** a vendor module exporting `socket` publishes the hash of that name, so a plain name is
a NID nobody hashed yet, not a second kind of import. Dropping names that do not decode
reported a module with dozens of imports as needing none. A standard name carries no
attribution, and reading it as library zero would misattribute every import. A stated count
cannot be walked wrong.

**Rejected:**
- A second, string-keyed resolver: two resolution paths for one kind of import.
- Skipping undecodable names: an empty import list that is false.
- A hasher made inside the parser: a different suffix resolves nothing, silently.
