# D683 - Borrowed packet sizes are cross-checks, never sources

**Status:** decided
**Date:** 2026-09-14

Another project's table of command-packet sizes is read only after orbistoun measures, never
instead of measuring. Every packet size in the knowledge base is measured on the hardware;
agreement is recorded as corroboration, and a disagreement records both figures and stays open.

**Why:** tables from other projects in this space derive sizes from what guests reserve, which
cannot see builders no title exercises and folds in that project's own extra dwords. A guest
that inlined a size at compile time never asks the library, so an oversized packet overruns a
reservation made in good faith. The disagreements are the information, and adopting a table
erases them.

**Rejected:**
- Adopting the table: inherits its known defects, including a marker dword the library does not
  write.
- Reconciling a disagreement by preferring either side: hides an open question.
