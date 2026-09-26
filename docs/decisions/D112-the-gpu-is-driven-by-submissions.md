# D112 - The GPU is driven by submissions

**Status:** decided
**Date:** 2026-09-26

`orbistoun-gpu::pipeline` takes a submitted command buffer and a one-method reader of guest
memory, and derives everything - shaders, addresses, stages - from what the guest wrote. A
shader is read as a window up to its end-of-program instruction; a window that runs out is
refused, not truncated. Shaders the guest registered by name and shaders inferred from register
writes both run, and every disagreement is reported. A shader that will not translate is
reported with its address and reason, never skipped.

**Why:** this hardware generation has no high-level graphics call to intercept. A truncated
shader is a genuine prefix that looks correct. Agreement between the two routes is the only
evidence the register table is right.

**Rejected:**
- A translator handed shaders by the emulator: nothing would ever hand it one.
- Stopping at the first route that answers: discards the only check on the register table.
- Skipping an untranslatable shader silently: a frame that drew nothing, unexplained.
