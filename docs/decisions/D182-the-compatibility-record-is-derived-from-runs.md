# D182 - The compatibility record is derived from runs

**Status:** decided
**Date:** 2026-08-21

A title's file under `compat/` holds its settings and its measured result, in one file and
never merged. Every measured field is derived from a trace; the grade ladder uses stages the
loader distinguishes and stops at `Entered`, ranking within it by distinct imports then calls.
A run in which unimplemented functions reported success is never recorded, first entry included.

**Why:** two files would disagree about which was current. A hand-written grade drifts with
optimism and nothing checks it. A spin accumulates calls without learning anything, so surviving
to the limit is an outcome, not a distance. A contaminated baseline could never be beaten by an
honest run.

**Rejected:**
- Hand-written grades such as "playable": aspirational fiction.
- Merging measurements like settings: a measurement does not override another.
- A `Sustained` rung: ranks the least informative run first.
