# D471 - The fault reporter verifies a page is readable before reading it

**Status:** decided
**Date:** 2026-09-02

Before copying the machine code around a fault address into a report, the reporter checks
that the memory is committed and readable, rather than assuming the faulting instruction
pointer always names mapped memory.

**Why:** on an execute fault the instruction pointer itself is the unmapped address, so
reading through it faults a second time inside the fault handler, which ends the process and
leaves no report at all.

**Rejected:**
- Assuming the faulting address is always readable because it belongs to "the instruction
  that just faulted": true of a data fault, false of an execute fault, and the failure mode
  is a silently unreported run rather than a wrong one.
