# D128 - The guest heap is the host allocator

**Status:** decided
**Date:** 2026-09-26

`malloc`, `memalign`, `operator new` and their relatives are served by the host allocator
through one `allocate(size, align)` path. A sixteen-byte header records the offset back to the
allocation, which is also its alignment, and `free` refuses a header it did not write.

**Why:** the address space is identity-mapped, so a host allocation is a guest allocation at the
same address. One path keeps the layout `dealloc` needs from allocation to release, and a program
mixing `new` and `malloc` sees one heap.

**Rejected:**
- A private arena: a second allocator to get wrong.
- A separate aligned-allocation path: a header mismatch becomes heap corruption far from its cause.
