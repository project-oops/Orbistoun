# D833 - A texture binds its mip chain, base level to last

**Status:** decided
**Date:** 2026-10-14

A sampled texture's `BindTexture` carries every level from the descriptor's `BASE_LEVEL` to its
`LAST_LEVEL`, finest first. Each level's texels are packed at its own extent: each side is the
level above's halved, and never less than one. The pipeline reads each level where its layout
places it in the chain (`chain_level`), as it already reads the base level. The Vulkan backend
uploads every level into one image, one copy region a level. The sampler then reaches every
level its mip filter and LOD range allow.

A chain whose levels cannot all be placed binds its base level alone, as before. So does a block
format, a cube or a volume, whose chains are not modelled.

**Why:** a texture bound as its base level samples that level at every distance. A minified
texture then reads fine texels where the guest built coarse ones, and gl1-probe's
`mipmap-levels`, which draws an 8x8 chain over 4x4 pixels, read level 0's red for level 1's green.
The levels' places are already modelled for colour targets and textures alike
(`SurfaceLayout::level_offset`, the mip tail), so reading them is the same arithmetic per level.

**Rejected:**
- Generating the coarse levels on the host: the guest's own levels are its pictures, not an
  average of its base level.
- Binding one image per level: a sampler then never interpolates between levels.
