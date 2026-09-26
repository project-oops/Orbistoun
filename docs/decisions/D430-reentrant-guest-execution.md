# D430 - A library-call handler re-enters guest code on a dedicated stack, on the calling thread

**Status:** decided
**Date:** 2026-09-01

A handler that must call back into a guest function pointer (for example, the guest's own
one-time-initialisation callback) enters the callback on a fresh stack reserved in its own arena,
on the thread already running the handler, and frees the stack when the callback returns.
Reserving a guest virtual range honours the guest's own hint address, falling back to an owned
arena only on conflict.

**Why:** The calling thread is mid-handler on the guest's own stack, so the callback needs a
stack of its own rather than corrupting that frame; the same thread, because a callback reads
thread-local state. A guest's own allocator computes its arena's extent relative to the hint
address it passed to reserve a range, so ignoring that hint and handing back an unrelated address
breaks the guest's own arithmetic.

**Rejected:** running the callback synchronously on the handler's current stack - conflicts with
the handler's own frame; always reserving from a fixed arena regardless of the guest's hint - the
guest's arena-size arithmetic then underflows.
