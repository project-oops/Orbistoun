# D687 - A record updates on not-worse-and-different

**Status:** decided
**Date:** 2026-09-14

A run replaces a title's stored record when it is comparable, ranks at least equal, and differs
in its key or its outcome; `beats` still answers whether a run improved. The measurement date is
not a difference.

**Why:** a record describes the latest reproducible run and is not a trophy: a guest that
reaches as far and stops somewhere else would otherwise leave the record naming a fault site
nobody reproduces. Counting the date would rewrite the record on every rerun and make its diffs
unreadable.

**Rejected:**
- Updating only on strictly better: the record goes stale on any equal-rank change.
- Updating on every run: churn that hides real changes.
