# D765 - A retail process has no sanitizer replacement

**Status:** decided
**Date:** 2026-10-08
**known_by:** assumed

`sceKernelGetSanitizerMallocReplaceExternal()` and `sceKernelGetSanitizerNewReplaceExternal()`
answer null.

**Why:** a title's `libc.prx` asks for both while it sets up its heap. It tests each answer for null
and reads a size-prefixed table through anything else (PPSA02664 `libc.prx` `+0x16252`). The table
is the allocator a sanitizer runtime installs in place of the title's own, and a retail title runs
without one. Null keeps the title's allocator. The placeholder was read as a table and faulted.

**Assumed:** that a retail process on the console answers null too. obSCEne is asked to measure it.
