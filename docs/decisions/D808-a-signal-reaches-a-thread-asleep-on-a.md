# D808 - a signal reaches a thread asleep on a lock

**Status:** decided
**Date:** 2026-10-12

A signal raised on a thread asleep in `scePthreadMutexLock` (or POSIX `pthread_mutex_lock`, which
answers through it) runs on that thread, as on a thread asleep in the other owned waits (D652). The
lock's state is released while the handler runs, and the thread then waits for the lock again. The
lock call marks the thread parked for the length of its wait, and a raise nudges lock waiters with
the others.

Grounded in FreeBSD's libthr, which the platform's threads follow: a contended mutex sleeps in
`_umtx_op`. A signal interrupts that sleep, its handler runs, and the lock is retried. D652's
measurements found cross-thread delivery into a thread asleep in an address or umtx wait, which is
the same sleep.

**Why:** PPSA03416's IL2CPP runtime stops the world by raising a signal on each thread
(`sceKernelRaiseException`, through its PS5Util module). Each handler acknowledges on
"SuspendSemaphore" and waits on "resumeEvent". The GC finaliser was asleep on a lock the collecting
thread holds. Its raise was refused, so it never acknowledged, the collector waited forever, and the
title's loading screen never ended (D806, D807 named the waits). With delivery, the collection
completes and the title loads past it, to a shader using the cube-map instructions.

**Rejected:**
- Granting the lock to the signalled thread so its handler can run: the handler acknowledges and
  parks without touching the lock, and taking it would break mutual exclusion.
