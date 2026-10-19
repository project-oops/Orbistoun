# D898 - A triangle fan is seeded, and drawn in chunks of a list

**Status:** decided
**Date:** 2026-10-19

A primitive shader drawing a triangle fan (`DI_PT_TRIFAN`, 5, `gfx103.json:692`) has its geometry
engine inputs seeded as for the other topologies (D730). Triangle `i` takes the fan's first vertex
and vertices `i + 1` and `i + 2`. The rotation puts the provoking vertex where the draw's
convention looks, as Mesa's index generation orders a fan (`u_indices_gen.py:268-271`):
`(0, i + 1, i + 2)` under the last-vertex convention, and `(i + 1, i + 2, 0)` under the first.

A fan that one subgroup holds is drawn whole. A longer one is split, as a list or strip is (D741).
Every triangle needs the fan's first vertex, which a run of the draw's indices holds only in the
first chunk, so each chunk is drawn as a triangle list. The host writes out that list's
thirty-two-bit ids: the fan's own indices, read from the guest, or counted from its first vertex
for a fan with none. The module adds the draw's index offset to an indexed fan's ids, as it does
to any fetched index (D843).

**Why:** Grand Theft Auto V draws an indexed fan of 2784 indices through a primitive shader, and
its graphics queue stopped there.

**Rejected:**
- Seeding each chunk with the fan's first vertex as an extra input: the module reads its vertex
  ids as one run of indices, and a list of ids written out needs no change to it.
