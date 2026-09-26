# D378 - Guest syscalls dispatch through a harvested number table

**Status:** decided
**Date:** 2026-08-29

A guest's raw syscall number is mapped to an implementation through a table
harvested from the kernel's own syscall header; an unrecognized number answers
the platform's own refusal code, and the two indirect syscall forms bind to
nothing.

**Why:** The mapping keeps every syscall number traceable to the header it came
from rather than to a hand-picked exception, which is where earlier silent
mismappings crept in. The indirect syscall forms carry another number as their
first argument, so binding them directly would perform the wrong call with
every following argument shifted by one. A kernel refuses an unrecognized
request instead of granting it, so answering a placeholder success would tell
a guest something happened that did not.

**Rejected:**
- Header parsing that only accepts one constant spelling convention: it drops
  most of a header's constants silently, caught only by the resulting count.
- Binding the indirect forms to a real implementation: their first argument is
  a syscall number, not a normal argument, so the bound call is wrong and every
  argument after it is shifted.
