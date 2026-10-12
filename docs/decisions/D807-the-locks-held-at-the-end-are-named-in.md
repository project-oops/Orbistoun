# D807 - the locks held at the end are named in the report

**Status:** decided
**Date:** 2026-10-12

When a run ends, the report lists every guest lock still held. Each entry gives what the lock was
made as, its handle, the guest thread that holds it by name, and the depth where it is held more
than once. Read beside the threads reported inside a lock call (D806), it names who a blocked
thread waits for.

**Why:** PPSA03416's loading screen never ends. With D806 the report showed its GC finaliser
inside `pthread_mutex_lock` and its main thread inside `sceKernelWaitSema`, but not who held the
lock. This list shows the main thread holding two statically initialised locks while it waits:
the shape of a collection that waits for the other threads to stop while holding the collector's
lock.

**Rejected:**
- Counting the waiters on each lock as well: the threads reported inside a lock call already say
  who waits, and a count adds bookkeeping to every acquisition.
