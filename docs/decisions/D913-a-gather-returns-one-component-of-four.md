# D913 - A gather returns one component of four texels

**Status:** decided
**Date:** 2026-10-20

`image_gather4_lz` is SPIR-V's `OpImageGather`. It returns the component that the mask's one bit
names, from each of the four texels a sample at its position would filter, in the order
(0, 1), (1, 1), (1, 0), (0, 0), which is the hardware's order as well. All four destination
registers are written. A gather of a flat image only is translated; more than one mask bit,
offsets, or sixteen-bit data are refused.

In a dispatch it reads the image a fetch would, through its sampler (D904).

**Why:** Grand Theft Auto V's fragment shader gathers with `image_gather4_lz ... dmask:0x1`.
