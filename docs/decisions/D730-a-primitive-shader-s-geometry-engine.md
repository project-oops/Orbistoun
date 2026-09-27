# D730 - A primitive shader's geometry-engine inputs are seeded per draw

**Status:** decided
**Date:** 2026-09-27

A primitive shader that reads the geometry engine's inputs - its system SGPRs and input VGPRs -
is translated once per draw geometry, with those inputs seeded where GFX10's non-passthrough
primitive shader receives them. Only a draw one wave holds whole is seeded: non-indexed, one
instance, a list of three-vertex primitives, no index offset. Any other draw, and a shader that
reads the inputs with no geometry given, is refused by name.

**Why:** radeonsi's primitive shaders size their output and find their vertices from these
inputs; read as zero they emit nothing and the draw counts as done. A shader that never reads
them - everything written for the open toolchain - is translated once, as before, so seeding
costs nothing where it is not used. Baking the counts in keeps the draw-data buffer the user
data's alone (D718).

**Rejected:**
- Seeding every primitive shader: a module per draw count for shaders that never look.
- The counts in the draw-data buffer: the stride is the user data's, and radeonsi uses most of
  it.
- Splitting a large draw into subgroups: the geometry engine's grouping is its own, and nothing
  here needs it yet.
