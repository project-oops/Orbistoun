# D030 - Backend seams are enforced by crate boundaries

**Status:** decided
**Date:** 2026-08-19

`orbistoun-gpu` holds command-stream and shader translation and does not depend on `ash`;
`orbistoun-gpu-vulkan` is the only crate that knows Vulkan exists.

**Why:** with no dependency there is nothing to leak, and `cargo` enforces that rather than
review. A second backend is a new crate rather than surgery on the translator.

**Rejected:**
- One crate with a module boundary: discipline instead of enforcement.
