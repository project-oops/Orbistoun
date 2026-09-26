# D458 - A one-shot instruction breakpoint captures a function's arguments at entry

**Status:** decided
**Date:** 2026-09-01

A diagnostic breakpoint can be armed on a guest instruction address; the first time it is
reached, the full register state is captured, the breakpoint disarms itself, and the guest
continues running normally.

**Why:** A guest-computed value that caused a fault elsewhere often cannot be read from the
guest's file at all, only from its running state at the point it is produced; a hardware
instruction breakpoint that fires once and then gets out of the way answers that without adding a
resume mechanism or the risk of an infinite re-trap.

**Rejected:** a breakpoint that re-arms and keeps trapping - needs a resume path and can wedge a
guest that hits the address in a loop.
