# D397 - An unset kernel release string refuses rather than answers a guess

**Status:** decided
**Date:** 2026-08-30

The kernel's own release string is a per-machine setting that is empty until
configured; while empty, a query for it is refused and reported once rather
than answered with an invented value.

**Why:** A guest branches on this string, so answering something plausible
sends it down a path chosen by a number nobody measured, producing a run that
looks correct without being one. Refusing is the honest answer to a question
this project has no basis to answer, and the reported refusal doubles as the
record of which guests actually ask.

**Rejected:**
- Answering a plausible placeholder version: risks silently routing a guest
  down the wrong branch with no visible sign anything was guessed.
