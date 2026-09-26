# D454 - Guest local time is presented as UTC

**Status:** assumed
**Date:** 2026-09-01

The guest-facing local-time conversion applies no timezone offset; local time and UTC are the
same value here.

**Why:** No timezone is modelled, and answering an invented offset would be less honest than
answering the one time value this project actually has grounds for. The breakdown itself follows
a cited exact calendar algorithm and the platform's own time-structure layout.

**Rejected:** guessing a plausible regional offset - fabricates a fact about the running machine's
configuration that nothing here establishes.
