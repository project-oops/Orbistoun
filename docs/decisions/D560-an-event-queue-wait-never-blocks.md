# D560 - An event queue wait never blocks

**Status:** assumed
**Date:** 2026-09-04

Waiting on an event queue delivers what is ready and reports zero delivered when nothing is, even
for an indefinite timeout. A delivered event uses the FreeBSD `struct kevent` layout, with `udata`
taken from the registration and `filter` written as zero.

**Why:** a blocked wait with no poster on another thread stops the run forever, and a hang
destroys a run's evidence where a busy loop only costs time. Zero delivered is what `kevent`
reports on a timeout, so the answer is still lawful. The target kernel is FreeBSD-derived, which
makes `kevent` the citable layout; `udata` echoes the caller's word, so only the registration
knows it.

**Rejected:**
- Blocking on a null timeout: the thread that would post the completion is often the blocked one.
- Leaving delivery unimplemented: the guest waits on a flip completion that was performed and
  never announced.
