# D293 - The turn dispatcher is separate from the naming model

**Status:** decided
**Date:** 2026-08-26

The dispatcher (`turn`, `experiment`, `axis`, `trial`) is `orbistoun-turn`;
`orbistoun-propose` holds only word proposal and its model. Each has its own error type.

**Why:** the two share no code, and a shim that runs a turn should not link a model runtime
and its network and tokeniser dependencies. A crate boundary makes the leak impossible and
lets cargo police it. A shared error type would be the coupling the split removes.

**Rejected:**
- A feature flag on one crate: hides the dependency instead of removing it.
