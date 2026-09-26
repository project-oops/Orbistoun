# orbistoun-nid

NID hashing and symbol-name resolution.

`NidHasher` is the forward direction (name to 64-bit hash) and `SymbolDb` the reverse (hash to
name). A hash is not invertible, so reverse resolution is pure lookup and an unknown NID stays
unknown. An unresolved NID is still useful output: the title needs it, and the trace says how
often and from where. [orbistoun-names](../orbistoun-names/) generates the candidates that fill
the database.

## Rules

- **The hash suffix is runtime data, not a source constant.** It keeps a bare magic value out
  of the tree and makes the hasher testable against any suffix without a recompile. Without a
  real suffix, names are correct and hashes are meaningless; the CLI warns when that is the
  case. The format is in [docs/SYMBOLS.md](../../docs/SYMBOLS.md).
- **A symbol database stores names only.** NIDs are derived, never stored, so the file cannot
  contain a pair that disagrees with itself.
