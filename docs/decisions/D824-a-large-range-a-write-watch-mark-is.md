# D824 - A large range a write-watch mark is taken of is write-protected while unwritten

**Status:** assumed
**Date:** 2026-10-13

When `orbistoun_mem::watch::mark` is taken of a range of 1 MiB or more whose pages are all
committed and plain read-write, the range is also made read-only. While it stays so, nothing has
written it, and `written_since` answers from its own record without asking the host. The first
write faults; the worker's fault handler asks `watch::write_faulted` before any other guard, the
range is made writable again and noted written, and the write runs, seen from then on as any
write is. Marks only guard once that handler is installed.

Anything other than a guest instruction that changes those pages releases the guard first, and
the pages are noted written: the guest's own protection change or unmapping (both pass through
`orbistoun_mem::platform`), another page guard (D717, D720), and the host writing guest memory
from its kernel, where a fault reaches no handler - the file, socket and APR reads, which call
`watch::release_overlapping` before handing a guest buffer to the host. At most 64 guards stand,
and none overlaps another.

**Why:** asking the host walks every page of the range in its kernel. PPSA25872 binds textures
spanning 34 MB, and the walk took 0.46 ms of every binding, though no check in a run found one
written. Under a guard the check takes no call, and binding fell from 0.5 - 0.6 ms a draw's
preparation to 0.05 - 0.08 ms.

**Rejected:**
- Asking the host at every check: exact, and the dominant cost of binding a large texture.
- Asking once a submission: saves only the repeated checks within one, and misses a CPU write
  the guest makes while the command processor waits on memory partway through it.
- Guarding without releasing for the host's kernel writes: a file read into a guarded texture
  pool fails rather than faulting, since the fault is raised in the kernel and reaches no
  handler.
