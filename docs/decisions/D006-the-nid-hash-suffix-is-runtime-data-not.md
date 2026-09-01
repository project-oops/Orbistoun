# D006 - The NID hash suffix is runtime data, not a source constant

**decided** · 2026-08-19

Keeps a bare magic value out of the tree, where it would be awkward to justify the
provenance of, and makes the hasher testable against arbitrary suffixes without a
recompile. Consequence: without `--suffix-hex`, symbol names are correct and the
NIDs shown are meaningless - the CLI warns when that is the case.

