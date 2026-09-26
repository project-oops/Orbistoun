# D476 - The errno table carries a published tier alongside its measured one

**Status:** assumed
**Date:** 2026-09-02

A value with no hardware measurement behind it, but a citable published source, is recorded
in its own tier alongside the measured errno table, rather than left absent or folded into
the measured one.

**Why:** reporting a wrong value, or the project's own unimplemented-call placeholder, for a
call that is fully implemented and whose outcome is known, is worse than a clearly labelled
published value. The measured tier's claim to record only observed values must stay exactly
true, not loosened to admit an unmeasured one.

**Rejected:**
- Answering a measured value that is wrong for this condition: a value a guest can act on
  incorrectly.
- Answering the unimplemented-call placeholder: the call is implemented and its outcome is
  known.
