# D871 - A two-channel 16_16 UNORM target is drawn through R16G16_UNORM

**Status:** decided
**Date:** 2026-10-14

A colour target whose `CB_COLOR0_INFO` names `COLOR_16_16`, `NUMBER_UNORM` and `SWAP_STD` holds two
normalised 16-bit channels a texel, red then green (`XY__`). It is four bytes a texel, placed by the
32-bpp layout, and drawn through `R16G16_UNORM`, its words moved in and out as they are held and
red and green written - as its `FLOAT` twin is through `R16G16_SFLOAT` (D847).

**Why:** `COLOR_16_16` is 5 and `NUMBER_UNORM` 0 (`gfx103.json`). Grand Theft Auto V's eighteenth
submission draws into such a target once its first seventeen run (D866, D868-D870).
