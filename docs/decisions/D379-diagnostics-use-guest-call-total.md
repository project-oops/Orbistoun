# D379 - Diagnostics use the guest's own call total, not its import count

**Status:** decided
**Date:** 2026-08-29

A report of what a guest imported is sized by the guest's own import count; a
diagnostic that forces or inspects a call is sized by the total count of
everything this project can answer, and the two are never the same variable.

**Why:** A single shared size silently excluded every by-name-resolved call
from every diagnostic, because the import count is deliberately the guest's
own, smaller number. A diagnostic that only reaches part of the program while
reporting no change reads exactly like a negative finding.

**Rejected:**
- One shared count for both uses: a report or a diagnostic then either counts
  things it should not, or misses things it should reach.
