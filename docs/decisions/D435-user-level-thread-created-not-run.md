# D435 - A newly created user-level thread is recorded, not run

**Status:** decided
**Date:** 2026-09-01

Creating a cooperative user-level thread records it and answers success without running its entry
point; the entry runs only once something yields to it.

**Why:** A cooperative thread does not run until yielded to, and this project has no scheduler
for it yet. Running the entry synchronously at creation would be honest neither to the guest's
own model nor safe - it would hang on the entry's first blocking wait, with nothing yet able to
resume the creator.

**Rejected:** running the entry point synchronously at creation to look further along - hangs the
first time the entry blocks.
