# D145 - Wavefront width is supplied, not inferred

**Status:** decided
**Date:** 2026-09-26

`Strategy::Predicated` carries a wavefront width, defaulting to 64, supplied by the caller from
the pipeline state. Mask instructions write through the mask at either width. Probes are
assembled in 64-lane mode, because width does not change the encodings.

**Why:** nothing in the instruction stream states the width, and inferring it from which mask
instructions appear guesses from an absence. A 32-lane shader narrows its mask with 32-bit
scalar instructions, which must reach the mask rather than an ordinary register.

**Rejected:**
- Inferring width from the shader: fails for any shader that has not yet touched its mask.
- Separate tables per width: the encodings are identical.
