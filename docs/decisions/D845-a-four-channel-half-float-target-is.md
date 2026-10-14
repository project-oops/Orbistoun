# D845 - A four-channel half-float target is drawn and written back as it is held

**Status:** decided
**Date:** 2026-10-14

A colour target whose `CB_COLOR0_INFO` names `COLOR_16_16_16_16`, `NUMBER_FLOAT` and `SWAP_STD`
holds four half floats a texel, red first, eight bytes. It lies by its own layout: for
`64KB_R_X`, 128 x 64-texel blocks with addrlib's 64-bpp equation (`tiling.rs`
`RX_BPP8_X_BITS`). It is drawn through a Vulkan `R16G16B16A16_SFLOAT` attachment whose image
and buffers are eight bytes a texel. Seeding it from guest memory and writing it back move two
guest words a texel untouched, as D773 and D844 do at four. Any other swap stays refused.

**Why:** `COLOR_16_16_16_16` is what Mesa targets a four-channel 16-bit format at
(`ac_formats.c` 527). Grand Theft Auto V's third draw writes a 240 x 135 target of it, once its
first two run (D843, D844).

The 64-bpp `64KB_R_X` equation was read off addrlib by `tools/addrlib/equation 64KB_R_X 8 32`
under the console's configuration. Unlike the one- and four-byte equations, it has a term above
the block: `y` bit 6 adds `0x800` inside it. The tool checks row-major blocks under those terms
against addrlib for every texel of a 32 x 32-block surface and a 5 x 3-block one. Blending on
`R16G16B16A16_SFLOAT` is required of every Vulkan implementation.

**Rejected:**
- Converting through an eight-bit attachment: a half float is not recoverable from eight bits.
- Reading the 64-bpp layout as the 32-bpp one at twice the width: the equations differ, and the
  term above the block exists only at this size.
