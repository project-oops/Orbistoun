# D100 - Fidelity levels are a differential oracle

**Status:** decided
**Date:** 2026-09-26

Three wavefront models exist: `Lane` (one invocation per lane, no mask), `Wavefront` (one
invocation simulates every lane) and `Subgroup` (the lane model plus a mask built with a
subgroup ballot). Generated programs run at two levels and must agree. A pinned level that
cannot run is an error, never a substitution.

**Why:** disagreement between two levels localises a bug to one shader and one instruction with
no hardware reference. The subgroup level is the lane model with and without a mask, so it
shares code rather than duplicating it. Levels differ in correctness, so a substitution renders
something wrong.

**Rejected:**
- A fallback ladder: silently drops a level.
- A separate third model: duplicates the lane model.
