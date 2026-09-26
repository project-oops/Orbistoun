# D227 - Diagnostics that intervene say so at the verdict

**Status:** decided
**Date:** 2026-09-26

Each diagnostic declares an `Effect`: it observes or it intervenes. A run under an intervening
diagnostic that reports progress prints a caveat at the verdict, and a diagnostic asked for but
applied zero times is printed at the verdict as having measured nothing. A setting may never
intervene. A guard is not trusted until it has been made to fail.

**Why:** the tooling can report success it did not establish exactly as a stub can. A moved fault
under an intervention may mean the guest accepted a wrong answer. A mechanism whose "did nothing"
output matches its "changed nothing" output is eventually read as an elimination. The caveat
belongs where the conclusion is drawn.

**Rejected:**
- A caveat on every instrumented run: noise people learn to skip.
- Counts in the conditions line only: read straight past.
- Listing effects separately from the registry: two lists that drift.
