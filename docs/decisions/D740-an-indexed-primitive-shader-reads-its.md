# D740 - An indexed primitive shader reads its vertex ids from the index buffer

**Status:** decided
**Date:** 2026-10-05

A primitive shader that reads the geometry engine's inputs (D730) is seeded for an indexed draw
too. Each vertex thread's vertex id is the index at its position in the draw's index buffer - the
raw index, as for a non-indexed draw it is the vertex's number from zero: radeonsi adds the base
vertex itself, from a user register. The module reads the index at run time, through the draw
buffers (D733) at the slot after the ones its program traces; the pipeline binds the draw's index
range there, `VGT_INDEX_TYPE` giving its width. An index-reading module is translated once per
draw shape, not once per index buffer's contents.

Each vertex thread takes one index, with no reuse between primitives: a list's primitive `i` takes
threads `3i` to `3i + 2`, as a non-indexed list does. Every vertex is computed from its own id, so
a vertex computed twice is the same vertex.

Eight-bit indices and primitive restart are refused by name.

**Rejected:**
- The index values compiled into the module: a module per index buffer's contents.
