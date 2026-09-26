# D396 - Kernel-boundary log lines are written when the event happens

**Status:** decided
**Date:** 2026-08-30

Events at the kernel boundary - a path resolved to nothing, an unanswered
request - are written to the log device as they occur; only a raw guest
syscall, whose dispatch path must not print (D381), is deferred to the closing
summary.

**Why:** A record and its reader must be alive at the same moment; writing
kernel-boundary events only after the guest stops produces a device with a
listener that has already been told the run is over. Writing them live is
what makes a log device function as one.

**Rejected:**
- Writing every event only in the end-of-run summary: correct information,
  delivered to a reader that is no longer listening.
