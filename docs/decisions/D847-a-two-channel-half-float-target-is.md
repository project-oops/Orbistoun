# D847 - A two-channel half-float target is drawn and written back as it is held

**Status:** decided
**Date:** 2026-10-14

A colour target whose `CB_COLOR0_INFO` names `COLOR_16_16`, `NUMBER_FLOAT` and `SWAP_STD`
holds two half floats a texel, red then green, four bytes. It lies by the 32-bpp layout. It is
drawn through a Vulkan `R16G16_SFLOAT` attachment whose image is the same format, so seeding
and write-back move the guest's words untouched, as D773, D844 and D845 do. A draw into it
writes red and green of its target mask. Any other swap stays refused.

**Why:** `COLOR_16_16` is what Mesa targets a two-channel 16-bit format at (`ac_formats.c` 500),
and `SWAP_STD` on two channels is `XY__` (592). Grand Theft Auto V's fourth draw writes a
1920 x 1080 target of it, once its first three run (D843, D844, D845). Blending on
`R16G16_SFLOAT` is required of every Vulkan implementation.

**Rejected:**
- Drawing it through an eight-bit attachment: a half float is not recoverable from eight bits.
- Keeping all four channels of the target mask: the attachment holds two, and a draw would be
  refused for channels its pixel shader need not export.
