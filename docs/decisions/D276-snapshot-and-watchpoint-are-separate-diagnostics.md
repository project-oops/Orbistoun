# D276 - Snapshot and watchpoint are separate diagnostics

**Status:** decided
**Date:** 2026-09-26

`ORBISTOUN_WATCH` copies a region and diffs it afterwards; `ORBISTOUN_WATCHPOINT` arms up to
four x86 debug registers and reports each access. A watchpoint report places the instruction
pointer *after* the access, never at it.

**Why:** the snapshot answers which bytes nobody wrote, for one copy and no platform code, and
is run first; the watchpoint answers who touched a word and when. Together they form a
mechanical pipeline: the snapshot names unwritten words, and those become watchpoints on the
next run. A data breakpoint is a trap that fires after the access, and naming the exact
instruction would need decoding guest code, which the provenance rules refuse; the report is
still bounded to one instruction.

**Rejected:**
- The watchpoint replacing the snapshot: costs a debug register and an exception per access for a question a copy answers.
- Reporting the access "at" the trapped address: off by one instruction, and wrong conclusions follow.
