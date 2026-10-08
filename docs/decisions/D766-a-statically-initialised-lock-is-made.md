# D766 - A statically initialised lock is made on first use

**Status:** decided
**Date:** 2026-10-08
**known_by:** published (FreeBSD libthr, `thr_mutex.c` and `thr_cond.c`)

A mutex word holding `PTHREAD_MUTEX_INITIALIZER` (0) or the adaptive initialiser (1) is made the
first time it is locked, try-locked or timed-locked, as libthr's `CHECK_AND_INIT_MUTEX` makes it.
The type is libthr's default, error-checking, or a normal lock for the adaptive initialiser. A
destroyed mutex holds libthr's destroyed marker (2) and answers `EINVAL`.

A condition-variable word holding `PTHREAD_COND_INITIALIZER` (0) is made the first time it is
waited on, signalled or broadcast (`CHECK_AND_INIT_COND`). A destroyed one holds the marker (1) and
answers `EINVAL`.

Each kind is made under one lock and re-read under it, so two threads that take the same static
lock first make a single lock between them.

**Why:** a title's own `libc.prx` guards its static constructors with static locks. Its
`__cxa_guard_acquire` and `__cxa_guard_release` aborted the title with "failed to acquire mutex"
and "failed to broadcast" when the lock answered a bad handle. il2cpp's collector lock is static
too, and its try-lock spun on the same refusal.

**Supersedes:** the refusal recorded on `mutex_at` ("a statically initialised lock names
nothing"). That answer was right not to grant success, but the platform makes the lock rather than
refusing it.
