# D705 - A completion is posted only for work that ran

**Status:** decided
**Date:** 2026-09-26

A fence or completion event for submitted GPU work is written only by carrying out that work.
The driver's completion-registration call stays unimplemented, and a queue a guest waits on that
nothing can post to is named as starved in the run report.

**Why:** a flip completes on acceptance because its only remaining job is scanout, which
orbistoun does not model; a submission's remaining job is execution, and completing work that
never ran tells the guest results exist that do not. There is no asynchronous GPU, so work
carried out at submit completes when the submit returns.

**Rejected:**
- Completing at acceptance, like a flip: claims results never produced and trades an honest wall
  for a busy loop.
- Registering the event with no producer: the same wait, arrived at more slowly.
