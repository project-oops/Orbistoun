# D251 - The guest filesystem is a manifest with per-title layers

**Status:** decided
**Date:** 2026-09-26

The guest-visible namespace mirrors the hardware's paths one to one and is declared in
`crates/orbistoun-fs/data/filesystem.toml`, each entry with its writability and how it is
known. A title's data is layered over that base in process, the base installed first, and a
mount is a stack resolved by walking it.

**Why:** the namespace is the only part a guest can observe, so it matches the hardware; where
the host keeps the bytes is unobservable, so it is whatever keeps one title's data together. A
path is added only when a guest asks for it, because an invented path is a fabricated platform
fact. Writability comes from the manifest so code and data cannot drift. A mount replaces
every root at its prefix, so the title layers rather than mounts, or the base hides it.

**Rejected:**
- A committed directory tree: carries no provenance and cannot hold empty directories.
- Merging layers on disk: a copy of the base per title, with no record of which file came from where.
- Paths added from expectation: a guest resolving an invented path gets a false answer instead of a failure.
