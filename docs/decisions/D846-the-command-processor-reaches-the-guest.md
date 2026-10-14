# D846 - The command processor reaches the guest's heap blocks

**Status:** decided
**Date:** 2026-10-14

The command processor reads and writes guest memory only where a lookup says the guest holds it. That
lookup now accepts a live block on the guest's heap as well as a kernel mapping. libc keeps every
block `malloc` and its family hand out, by start and size, and drops it when it is freed.
`orbistoun_libc::heap_block_holds` answers whether a range lies wholly inside one. The worker's
read and write lookups take either answer.

**Why:** `malloc` is served from the host heap (D128), outside every mapping the kernel records. A
title that gives the GPU a fence or label it allocated on its heap had its first submission stopped:
PPSA21564's first `RELEASE_MEM` targeted such a block, the command processor refused it as out of
bounds, and fourteen submissions waited behind it. On the console that title works (D709), so its
GPU writes the title's heap. A live block is memory the guest holds and writes itself, so the
command processor writing it on the guest's behalf reaches nothing the guest could not.

**Rejected:**
- Any committed host page: it would let a stale or wrong address write into orbistoun's own memory.
- The fixed heap arena (`ORBISTOUN_HEAP_BASE`): a diagnostic that changes the program, and it spills
  to the host heap when its 64 MiB run out, as PPSA21564's 145,297 allocations did.
