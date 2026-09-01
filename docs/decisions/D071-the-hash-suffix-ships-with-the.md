# D071 - The hash suffix ships with the repository

**decided** · 2026-08-19 · at the user's direction

Reversing the second half of D006. The suffix is embedded and used by default; nobody
has to supply anything.

**It is not a key.** It decrypts nothing, signs nothing, authenticates nothing, and
protects nothing. It is a salt on a name-mangling hash, and its only effect is to make
the mapping from names to identifiers non-obvious. It is not per-title, not
per-firmware, and not per-generation - one constant throughout. Every emulator of this
target necessarily contains it, because resolving imports is the central act of
high-level emulation.

Requiring a user to provide it would add a setup step that protects nothing and helps
nobody.

**D006's first reason still holds**, so the value lives in a documented data file rather
than a Rust literal. `crates/orbistoun-nid/data/hash-suffix.toml` states what it is,
what it is not, why it is present, and how it verifies itself. An unexplained hex blob
in source is the artefact that makes provenance questions awkward; a constant with its
derivation written beside it is not. It is embedded at build time, so a portable
single-binary build carries it with no external file to lose.

**It verifies itself, and the tool checks rather than trusts.** Hashing published C
library names with it matches dozens of real imports; a wrong value matches none.

