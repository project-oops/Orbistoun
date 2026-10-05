# D739 - The shader target is the console's own revision

**Status:** decided
**Date:** 2026-10-05

The generators assemble for `gfx1013`, the console's own part, not `gfx1030`. oops-mesa's device
information reports `GFX1013` inside the Navi family, and the shaders a title runs are compiled
for it: radeonsi's compiler emits `v_mad_f32` and `v_mac_f32` there, and `gfx1030` removed
`v_mac_f32`, so the reference assembler refused to name an instruction SuperTuxKart and Craft both
run.

Re-recording every table for `gfx1013` changed no name or layout the two revisions share; it added
the three that only `gfx1013` has. The published RDNA2 reference describes `gfx1030` and stays the
document an encoding is looked up in. D139's rule is unchanged: instructions are keyed by name, and
the target is defined once.
