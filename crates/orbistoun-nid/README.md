# orbistoun-nid

NID hashing and symbol-name resolution.

**Models:** the forward direction (name to 64-bit hash, `NidHasher`) and the reverse
(hash to name, `SymbolDb`). A hash is not invertible, so reverse resolution is pure
lookup and unknown NIDs stay unknown.

**Deliberately fakes:** nothing. An unresolved NID reports as unknown rather than
being guessed at - and that is still useful output: you know the title needs it, how
often it is called, and from where.

**Design note.** The hash suffix is **runtime data, not a source constant**. Two
reasons: it keeps a bare magic value out of the tree, where it would be hard to
justify the provenance of, and it makes the hasher testable against any suffix
without a recompile.

Consequence worth knowing: without a real suffix, names are correct and hashes are
meaningless. The CLI warns when that is the case. Format in `docs/SYMBOLS.md`.

A symbol database file carries names only - NIDs are *derived*, never stored, so the
file cannot contain a pair that disagrees with itself.

**Status:** done. Hashing and lookup are tested for stability, suffix sensitivity and
unknown handling, and a database loads from disk - `--symbols-db` names 636 symbols today.
