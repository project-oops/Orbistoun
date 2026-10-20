# D935 - A depth target only cleared samples as its clear value

**Status:** decided
**Date:** 2026-10-20

A draw may sample a depth target that a clear was asked of and that no draw has drawn into. It
reads the cleared depth everywhere, or the cleared stencil where it samples the stencil plane
(D927). The backend binds one texel of that value, which reads the same wherever it is sampled
and whatever the sampler clamps or filters. A target neither drawn into nor cleared is still
refused, since nothing says what it holds.

A depth-bounds test (`DB_DEPTH_CONTROL.DEPTH_BOUNDS_ENABLE`) keeps a fragment whose stored depth
lies within `DB_DEPTH_BOUNDS_MIN` and `_MAX` (`gfx10.json`, 0x28020 and 0x28024), as Vulkan's does.
The bounds are clamped to `[0, 1]`, where a depth target's depths lie and where Vulkan takes them.
A device without `depthBounds` refuses the test by name.

**Why:** Grand Theft Auto V's loading-screen lighting samples the shadow cubes of lights that cast
nothing, each cleared and never drawn, and limits each light's pixels to its volume with a
depth-bounds test. The backend refused 9 of those binds and 14 of those tests, so the segment drew
nothing.
