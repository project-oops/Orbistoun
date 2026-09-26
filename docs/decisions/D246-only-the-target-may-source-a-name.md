# D246 - Only the target may source a name

**Status:** decided
**Date:** 2026-08-25

An existence fact from a probe transcript is graded like any other: `Measured` only when the
operator asserted the run was on the hardware being emulated, `Assumed` otherwise. Only a
`Measured` existence fact may source a name.

**Why:** a stand-in's symbol table comes from mined name lists, so a `present` from one is that
list speaking. Ungraded, it would reach the strongest provenance this project has. A stand-in run
still exercises the reader and compares implementations; it cannot mint a name.

**Rejected:**
- Ungraded existence facts: launders mined lists into names.
- Grading on "real hardware": real hardware that is not the target is still a stand-in.
