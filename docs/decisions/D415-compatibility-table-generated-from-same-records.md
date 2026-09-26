# D415 - The compatibility table is generated from the recorded runs

**Status:** assumed
**Date:** 2026-08-31

The compatibility document is generated from the recorded run data using the
same ranking logic the terminal listing prints, as its own command separate
from the run that records the data, so the two views cannot disagree.

**Why:** A hand-written or independently computed document drifts from what
running the corpus actually measured. Generating it from the same ranking
function the terminal view already uses means a reader can regenerate the
table without re-running anything, and the two views stay consistent by
construction rather than by discipline.

**Rejected:**
- Writing the document by hand from the terminal output: drifts from the
  underlying records the moment either changes independently.
- Computing the document's ranking separately from the terminal view's: two
  implementations of the same ranking can silently disagree.
