# D743 - A half-border clamp samples a saturated coordinate through a border sampler

**Status:** decided
**Date:** 2026-10-05

`SQ_TEX_CLAMP_HALF_BORDER` (4) is GL's `GL_CLAMP`: radeonsi and the SDK's GL layer both write it
for that wrap (`si_state.c:1934`, oops-sdk `gl_state.c:gl_hw_wrap`). The coordinate is clamped to
`[0, 1]`, so a linear filter at the edge blends the edge texel with half the border colour. A host
has no such sampler. Mesa's own form for a driver without it is the definition used here: the
shader saturates that coordinate, and the sampler clamps to the border when both filters are
linear and to the edge otherwise (`st_program.c:844-849`, `samplerobj.h:158-183`).

A pixel shader is translated per draw with the axes to saturate for each texture slot it samples,
read from the sampler descriptor that draw binds. The sampler takes the descriptor's built-in
border colour (`SQ_IMG_SAMP_WORD3.BORDER_COLOR_TYPE`); a half-border clamp naming the border
colour table (`REGISTER`) is refused, since its colour is not read.

**Rejected:**
- A border sampler without the saturate: equal inside `[0, 1]`, and fades to the border colour
  past it where the hardware holds the half-blended edge.
- A clamp-to-edge sampler: drops the border colour's half at the edge, the visible difference
  `GL_CLAMP` exists for.
