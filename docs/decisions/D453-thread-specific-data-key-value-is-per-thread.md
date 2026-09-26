# D453 - A thread-specific-data key's value lives in a per-thread table, its destructor unused

**Status:** decided
**Date:** 2026-09-01

A POSIX thread-specific-data key is a small integer from a monotonic counter; the value bound to
it per thread lives in a table local to that thread. A destructor registered at key creation is
recorded but never invoked.

**Why:** A guest thread is a host thread here, so a per-thread table gives each thread its own
value naturally, and a thread that never set a key correctly reads back empty. Nothing yet tears
a guest thread down through this layer, so there is no point in the guest's lifecycle at which a
destructor could run.

**Rejected:** invoking a destructor at some approximation of thread exit - nothing here currently
detects that exit, so it would run at the wrong time or not correspond to a real teardown.
