# D770 - A title's libc is told its heap is not traced

**Status:** decided
**Date:** 2026-10-08
**known_by:** assumed

`sceLibcHeapGetTraceInfo(info)` clears the 32-bit switch at `+0x0c` of the caller's 32-byte
block. It writes two pointers, at `+0x10` and `+0x18`, to two zeroed words orbistoun owns. It
answers 0, and leaves the rest of the block as the caller filled it.

**Why:** a newer title `libc.prx` (PPSA28061) calls it from `_malloc_init`, on a block filled from
a template. It reads back the switch, which turns heap tracing on when non-zero, and the two
pointers, which it keeps. It writes zero through the first at once, and writes its trace state
through the second when tracing is on. Unanswered, the first pointer stayed null and libc wrote to
address zero. That stopped PPSA28061 at 197 calls once its needed modules started before the entry
(D767).

**Assumed:** that a retail process runs without the heap tracer, so the switch is zero there too,
and that the words the console names are ordinary memory. obSCEne is asked what the call writes.

**Also here:** `sceKernelMapNamedFlexibleMemoryInternal`, which the same `_malloc_init` maps its
heap through next. It is called in the named mapping's shape, with flags `0x8000`, and maps as that
call does.
