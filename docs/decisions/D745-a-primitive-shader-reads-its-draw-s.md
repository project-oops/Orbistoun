# D745 - A primitive shader reads its draw's vertex count from the draw data

**Status:** decided
**Date:** 2026-10-05

A primitive shader seeded with the geometry engine's inputs (D730) is translated once per way of
assembling primitives and width of index, not once per draw. Each draw's vertex count travels in
the draw-data buffer (D718) as one more word after the geometry stage's user data, and the module
computes `gs_tg_info`, `merged_wave_info` and each lane's input VGPRs from it at run time: the
primitive count follows from the vertex count by the baked assembly, and every seeded draw's
first vertex is zero. This replaces D730's counts baked into the module; what D730 seeds, and for
which draws, is unchanged.

**Why:** a translated module simulates every lane and is megabytes of SPIR-V, which the driver
takes up to a minute to compile. A module per draw count made every new draw size a new compile -
CRFT00001's terrain chunks are each a different size, so the kept pipelines (D744) never held
them, and two such compiles took 120 s of a 120 s run. D730 rejected the counts in the draw-data
buffer because the user data filled its stride; the stride now has room for them.

**Rejected:**
- Specialisation constants for the counts: the driver compiles each specialisation as its own
  pipeline, which is the cost being removed.
- Push constants: shared by every workgroup of a batched dispatch (D718).
