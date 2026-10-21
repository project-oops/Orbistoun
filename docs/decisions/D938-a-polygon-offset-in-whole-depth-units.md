# D938 - A polygon offset in whole depth units pins depth to the range end

**Status:** decided
**Date:** 2026-10-21
**known_by:** measured

With `PA_SU_POLY_OFFSET_DB_FMT_CNTL` never written (zero), the constant term counts whole depth
units and the result is clamped to `0..1`. On a `Z_32_FLOAT` target an offset of -10 reads back
0.0 and +10 reads back 1.0 (obSCEne REQ-20261020T2010Z-gt24,
`report-20261020-gt24-poly-offset.log` rows 382 and 391). With `0x1e9`, the RADV float form, the
same offsets move a depth of 0.5 by ten float steps, as D32_SFLOAT depth bias does (rows 409 and
418).

An offset of magnitude one or more in those units, with no slope term and no clamp, moves every
depth past an end of the range. The draw is built with its viewport's depth range collapsed onto
that end: 0.0 for a negative offset, 1.0 for a positive one. Clipping runs in clip space before
the range applies, so the draw keeps the primitives it had. Every other offset in whole units is
refused by name.

**Why:** Grand Theft Auto V's loading-screen lighting draws with offsets of -10 and +10 and the
format register left at zero. Vulkan's depth bias cannot give whole units on a float target
without `VK_EXT_depth_bias_control`'s `floatRepresentation`, which the development device lacks.

**Rejected:**
- Writing the offset depth from the pixel shader: it changes every translated pixel shader that
  meets an offset, for a case the viewport answers exactly.
