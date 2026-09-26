# D344 - Guest threads park at the trampoline

**Status:** decided
**Date:** 2026-08-27

Suspending a title raises a process-wide park flag, and each guest thread parks at its next
pass through the import trampoline. The check is a load and a branch, with no allocation or
lock. The run report counts how many live threads parked, and ending a title clears the flag.

**Why:** suspending a thread at an arbitrary instruction can catch it holding the host runtime's
heap lock and deadlock the worker. A thread stopped in the trampoline is in our code and holds
no guest lock. A thread that never calls an import never parks, so the count makes that hole
visible. A flag left set would park the next run's threads.

**Rejected:**
- Suspending threads from outside: an occasional unrecoverable deadlock.
- Reporting "suspended" without a count: hides threads that kept running.
