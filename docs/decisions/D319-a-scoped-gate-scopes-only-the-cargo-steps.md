# D319 - A scoped gate scopes only the cargo steps

**Status:** decided
**Date:** 2026-08-27

`check --only "<crates>"` narrows the cargo steps to those crates, with clippy run
`--no-deps`; the repository checks - provenance, decision numbers, prose, generated numbers,
the symbol audit and the tables - always run whole-tree. A scoped run never prints
`all checks passed` and names what it did not compile, lint or test.

**Why:** several sessions share the tree, and one half-written crate otherwise blocks every
verification. The repository checks are facts about the repository, not a crate. Clippy lints
every workspace crate it builds from source, so without `--no-deps` another crate's finding is
reported under the scope. Green is what a reader takes as permission, so a subset must not
borrow it.

**Rejected:**
- Scoping the repository checks: the run would claim what it had not checked.
- Exempting formatting or docs from the scope: they read the whole tree and fail on others' files.
