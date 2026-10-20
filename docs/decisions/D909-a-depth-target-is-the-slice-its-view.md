# D909 - A depth target is the slice its view names

**Status:** decided
**Date:** 2026-10-20

A depth target is the slice that `DB_DEPTH_VIEW.SLICE_START` names: bits 10:0, with the high two in
12:11 (`gfx103.json`). Each slice is its own host attachment. Its id carries the slice in the
thirteen bits above the base address; slice zero keeps the id the base alone gave. A dispatch's
one-layer view of layer L of a depth array (`BASE_ARRAY` = `DEPTH` = L) names slice L's target.

**Why:** Grand Theft Auto V draws two 1024 x 1024 depth slices of one array and reads slice one
back in a compute dispatch. With one attachment per base address, both slices were drawn into one
target.

**Rejected:**
- Drawing every slice into one attachment and reading it as any layer: a later slice's draws
  overwrite an earlier one's.
