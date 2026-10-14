# D853 - A four-channel 32-bit float target is drawn and written back as it is held

**Status:** decided
**Date:** 2026-10-14

A colour target whose `CB_COLOR0_INFO` names `COLOR_32_32_32_32`, `NUMBER_FLOAT` and `SWAP_STD`
holds four 32-bit floats a texel, red first, sixteen bytes. It lies by its own layout: for
`64KB_R_X`, 64 x 64-texel blocks with addrlib's 128-bpp equation (`tiling.rs`
`RX_BPP16_X_BITS`). It is drawn through a Vulkan `R32G32B32A32_SFLOAT` attachment of the same
format, sixteen bytes a texel in its image and buffers, so seeding and write-back move the
guest's four words a texel untouched (D773, D845). Any other swap stays refused.

**Why:** `COLOR_32_32_32_32` is what Mesa targets a four-channel 32-bit format at
(`ac_formats.c` 529), and `SWAP_STD` on four channels is `XYZW` (612). Grand Theft Auto V draws
into a 1 x 1 target of it, past its 3840 x 2160 integer one (D852).

**Open:** Vulkan does not require blending on `R32G32B32A32_SFLOAT`, as it does not on D844's
`R32_SFLOAT`. The desktop hosts orbistoun runs on offer it; a host without it would fail the
pipeline rather than draw without blending.

**Rejected:**
- Converting through a half-float attachment: a 32-bit float is not recoverable from sixteen
  bits.
