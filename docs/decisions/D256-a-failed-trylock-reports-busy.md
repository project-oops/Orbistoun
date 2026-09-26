# D256 - A failed trylock reports busy

**Status:** decided
**Date:** 2026-08-25

A try-lock that cannot take the mutex answers a distinct busy result, never success and never
the invalid-argument placeholder.

**Why:** reporting that the lock could not be taken is the whole purpose of a try-lock, and a
guest branches on it. Success would send the guest into a critical section it does not hold,
with nothing in a trace to say so. A held lock is the ordinary outcome, not a caller error,
so a trace must tell it apart from a bad pointer.

**Rejected:**
- Answering success: silently breaks mutual exclusion.
- Reusing the invalid-argument code: a trace cannot tell a busy lock from a bad handle.
