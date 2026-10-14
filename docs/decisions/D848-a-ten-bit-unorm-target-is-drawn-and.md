# D848 - A ten-bit unorm target is drawn and written back as it is held

**Status:** decided
**Date:** 2026-10-14

A colour target whose `CB_COLOR0_INFO` names `COLOR_2_10_10_10`, `NUMBER_UNORM` and `SWAP_STD`
holds red, green and blue in ten bits each from the low end, and alpha in the top two, four
bytes a texel. It lies by the 32-bpp layout. It is drawn through a Vulkan
`A2B10G10R10_UNORM_PACK32` attachment of the same format, whose bits are laid out the same way,
so seeding and write-back move the guest's words untouched (D773, D844, D847). Any other swap or
number type stays refused.

**Why:** Mesa targets a format of 10-10-10-2 bits, red first, at `COLOR_2_10_10_10`
(`ac_formats.c` 536), and `SWAP_STD` on four channels is `XYZW` (612). Grand Theft Auto V's fifth
draw writes a 32 x 32 target of it, once its first four run. Blending on
`A2B10G10R10_UNORM_PACK32` is required of every Vulkan implementation.

**Rejected:**
- Drawing it through an eight-bit attachment: two bits of each colour channel would be lost on
  write-back.
