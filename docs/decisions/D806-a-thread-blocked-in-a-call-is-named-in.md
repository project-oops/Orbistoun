# D806 - a thread blocked in a call is named in the report

**Status:** decided
**Date:** 2026-10-12

Each host thread publishes the import it is inside to a fixed table: one relaxed store when a
call starts and one when it ends, in the same places the thread-local record of the call is kept
(D621). A thread claims a slot on its first call. The table holds 512 threads, and a thread past
them is not published. The run report reads the table when the run ends. A guest thread with no
call in the recorded window, but inside one now, is reported as inside that call.

**Why:** the recorded window holds the run's last 48 calls, so a thread blocked in a call made
long before the end was reported only as "no call in the last 48 of the run". That says nothing
about where it waits. PPSA03416's loading screen never ends, and its threads were all reported that
way. With the table, the report says the main thread is inside `sceKernelWaitSema`, the job workers
inside `sceKernelWaitEventFlag`, and the GC finaliser inside `pthread_mutex_lock`.

**Rejected:**
- A lock-guarded map from thread to call: a lock on every guest call costs more than the guest's
  own work in the hottest titles.
- Reading the other threads' thread-locals: no safe way exists, and the thread may be running.
