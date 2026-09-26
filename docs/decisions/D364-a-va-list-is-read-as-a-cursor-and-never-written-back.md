# D364 - A va_list is read as a cursor and never written back

**Status:** decided
**Date:** 2026-08-29

The `v` formatting functions read the guest's System V `va_list` - register save area, then
overflow area - through the same `render_format` as the register forms, which take an argument
source trait. The cursor advances on the host side only.

**Why:** the register forms see only the arguments the trampoline caught and refuse a longer
format; a `va_list` reaches every argument. One renderer keeps both spellings producing the
same string. The standard leaves `ap` indeterminate after the call, so not advancing the
guest's copy is unobservable and the safer of two permitted behaviours.

**Rejected:**
- A second formatter: drifts from the first.
- Writing the advanced cursor back: a guest-memory write nothing needs.
