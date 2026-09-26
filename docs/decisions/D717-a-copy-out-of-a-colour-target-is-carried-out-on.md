# D717 - A copy out of a colour target is carried out on first touch

**Status:** decided
**Date:** 2026-09-24

A command-processor copy whose source is exactly the whole pending target is deferred: the
device frame is snapshotted in queue order, the target's kept bytes are held, and the
destination's host pages are made inaccessible. The first access to them faults, and the handler
carries the copy out and retries; command-processor work touching the destination carries it out
first, and a later copy to the same destination drops it unread.

**Why:** the open-toolchain GL context copies its whole target into a readback buffer at the end
of every submission, and writing the frame back first held a GL port at two or three frames a
second. The deferred bytes are those the hardware's copy leaves, at the first moment anything
can observe them. Where the destination cannot be guarded, in a region of mixed protection or on
a host without the fault path, the copy runs eagerly.

**Rejected:**
- Writing back before every copy: exact, and a readback and retile per submission.
- Deferring without a guard: a reader sees stale bytes.
- Changing the SDK to copy on demand: orbistoun must be exact for any guest.
