# D177 - A guest stop is its own run outcome

**Status:** decided
**Date:** 2026-08-21

A guest that calls `abort` or `exit` ends the run through `orbistoun-core::stop`, a handler the
worker installs. The trace is persisted and records the stop as an outcome distinct from a fault
and from the time or call limit.

**Why:** a `noreturn` function that returns runs into the compiler's trap and reports an illegal
instruction, the opposite of what happened. How to stop is the worker's business, and the worker
sits above the subsystem crates, so the dependency is inverted through a handler.

**Rejected:**
- Letting the stub return: a false fault report.
- Subsystems ending the process directly: the trace is lost.
