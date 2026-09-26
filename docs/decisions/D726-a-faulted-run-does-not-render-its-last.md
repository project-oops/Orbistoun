# D726 - A faulted run does not render its last submission

**Status:** assumed
**Date:** 2026-09-26

A run that ends at the time limit, the call budget or a stop the guest made itself hands its
last submission to the backend before its trace is persisted. A run that ends in a fault does
not: the fault report is written from the handler on the faulting thread, and no backend work
is started there.

**Why:** the handler runs in an exception context on a guest thread that may hold a device,
heap or driver lock, and rendering from it could wait on that thread forever and lose the trace
the handler exists to save. Since D712 every draw is carried out at submit, so a faulted run's
submissions have already reached the backend; only the report's final render is skipped.

**Rejected:**
- Rendering from the fault handler: a fault inside the driver or the heap would deadlock the
  report.
- Deferring the render to a second process after the fault: the guest memory it reads is gone
  with the faulted process.
