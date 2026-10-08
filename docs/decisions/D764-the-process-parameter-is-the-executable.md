# D764 - The process parameter is the executable's own segment

**Status:** decided
**Date:** 2026-10-08
**known_by:** guest-observed

`sceKernelGetProcParam()` answers the address of the executable's `PT_SCE_PROCPARAM` segment, as
placed and relocated. An executable without one gets null.

**Why:** every commercial title in the corpus ships its own `libc.prx`. Its `_malloc_init` calls
this first. It reads a size of at least `0x40` at `+0`, then the libc parameters through the
pointer at `+0x38`, before it sets up the heap that every allocation inside `libc.prx` comes from.
PPSA02664's segment has exactly that shape. It states `0x60` bytes and `"ORBI"` at `+8`, and its
relative relocations fill `+0x30` to `+0x48`; the `+0x38` slot points at the eboot's libc
parameters. Answered with the placeholder, `_malloc_init` dereferenced it.

**Rejected:**
- A block orbistoun builds itself, like the system-version block: the libc parameters and the
  allocator replacement the title declares are the title's own, and only its segment holds them.
