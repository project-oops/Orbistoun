# D715 - An orphaned worker ends its process

**Status:** assumed
**Date:** 2026-09-24

When the worker's control channel ends without a `Shutdown` before it, the worker ends its whole
process with `EXIT_ORPHANED` (3) and a line on stderr saying why.

**Why:** the only peer holds the pipe open until it sends `Shutdown`, so an end without one
means the parent exited, crashed or was killed, and the input thread notices even while the main
thread is in guest code. Guest code cannot be unwound from outside, and the trace is written as
the run goes, so nothing recorded is lost. An orphan otherwise runs on with nobody to report to
and holds the executable open.

**Rejected:**
- A job object with kill-on-close: one platform only, and new unsafe code.
- Finishing queued work first: the case that matters never returns.
