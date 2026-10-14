# D841 - A handler short of stack runs on a lent host thread

**Status:** decided
**Date:** 2026-10-14

A handler runs on the stack of the guest thread that called it. A handler whose own work is deep
asks `orbistoun_kernel::thread::with_room` for the stack it needs. When the calling thread's guest
stack has that much left below the current frame, the work runs inline, as every handler always
has. When it does not, the work runs on a scoped host thread with a 64 MiB stack while the guest
thread waits for it. The lent thread starts at the guest thread's logical time, and the guest
thread's clock moves up to where the work left it (D735). A host that refuses the thread gets the
work inline.

`sceAgcDriverSubmitDcb` and `sceAgcDriverSubmitCommandBuffer` ask for 4 MiB. Preparing a
submission decodes and translates shaders and walks the command buffer.

**Why:** a stack the title supplies (`scePthreadAttrSetstack`) is sized for the console's
libraries, whose submit is a thin hand-over. PPSA21564's render thread runs on its own stack, and
its first submission overflowed it inside orbistoun's translator. The guest's stack is the guest's:
growing it would move memory the title placed, and the guard below it is the title's overflow check.

**Rejected:**
- Running every handler on a host stack: most handlers are shallow, and a thread switch for each
  call is a cost the common case does not need.
- Reserving extra space below a supplied stack: the memory below it is the title's, not
  orbistoun's.
