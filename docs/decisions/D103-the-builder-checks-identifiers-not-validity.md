# D103 - The builder checks identifiers, not validity

**Status:** decided
**Date:** 2026-09-26

`Builder::check` verifies before a module leaves the crate that every identifier used is
defined, that nothing is defined twice, and that the declarations section defines before it
uses. Function bodies are checked only for "defined somewhere". Anything else is left to the
authoritative validator.

**Why:** the builder hands out every identifier, so it alone can say which never got a meaning,
and it turns a driver fault into a named error. Control flow names labels before they exist, so
ordering cannot apply inside a function.

**Rejected:**
- A second SPIR-V validator: second-guessing the authoritative one.
- Define-before-use inside functions: rejects every forward branch.
