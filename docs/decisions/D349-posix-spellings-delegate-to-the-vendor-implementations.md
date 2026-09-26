# D349 - POSIX spellings delegate to the vendor implementations

**Status:** decided
**Date:** 2026-09-26

`orbistoun-posix` declares the POSIX library's names and serves each one with a vendor-named
twin by delegation: a table maps the name to the twin's function pointer and arity, looked up
from the implementing crate. Where the spellings differ in arity they have separate entry
points, and a failure returns the project's placeholder rather than an errno.

**Why:** a NID is the hash of a name, so a behaviour already implemented is unreachable until
its second spelling is declared. Delegation copies nothing, so the two cannot drift. A shared
body cannot see which arity called it and reads an argument the shorter spelling never passed.
An invented errno is a plausible wrong answer; the placeholder sends a switching caller to its
default branch.

**Rejected:**
- Copied implementations per spelling: drift.
- A rule-generated mapping: "mostly mechanical" binds some names wrongly.
- One body for two arities: reads a register the caller never set.
