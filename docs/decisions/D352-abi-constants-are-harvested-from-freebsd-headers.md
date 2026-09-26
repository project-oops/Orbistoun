# D352 - ABI constants are harvested from FreeBSD headers

**Status:** decided
**Date:** 2026-09-26

`orbistoun-gen constants <checkout>` harvests `#define` constants from a fixed list of FreeBSD
headers into `crates/orbistoun-hle/data/abi-constants.toml`, stamped with the revision
`git rev-parse HEAD` reports; the gate regenerates and diffs it. Code reads values from the
table and never retypes them. Composite expressions and function-like macros are skipped.

**Why:** implementing needs interface numbers, and a public header constant is an ABI fact like
a symbol name. The values are FreeBSD's, so each is published about FreeBSD and assumed about
the guest. Reading from the table keeps a harvested constant distinguishable from a remembered
one. Taking the revision as an argument let a hand-edited header regenerate to itself; a
checkout that is not a repository is refused. Absent the checkout, the gate step warns and
passes.

**Rejected:**
- Constants typed into Rust: untraceable.
- A revision passed on the command line: the header could name any source.
- A directory walk: hundreds of constants nobody asked for.
- Evaluating composite expressions: reproduces a decision instead of reading a number.
