# D868 - A half-float target is drawn as floats and held as halves

**Status:** assumed
**Date:** 2026-10-14

A colour target whose `CB_COLOR0_INFO` names `COLOR_16`, `NUMBER_FLOAT` and `SWAP_STD` holds one
IEEE half float a texel, red. It lies by the two-byte layout, as a `COLOR_16` `UINT` target does
(D852), and a host carries each texel as a word with the half in its low sixteen bits. A drawer
draws it through `R32_SFLOAT`: a half is widened to its float exactly where words enter the
attachment - seeded, reloaded, or filled uniformly - and a float is rounded to the nearest half, a
tie to the even, where they leave it - read back, by rows, or through a snapshot. Draws and
blending run in 32-bit float.

**Known by:** assumed. That the hardware rounds a half-float target's result to the nearest, ties
to even, is the IEEE default and what Vulkan's `R16_SFLOAT` attachments do on the hosts here; it is
not measured on the console. Blending in 32 bits where the hardware may blend nearer 16 can differ
in the last bit of a half.

**Why:** `NUMBER_FLOAT` is 7 and `COLOR_16` is 2 (`gfx103.json`). Grand Theft Auto V's second
submission draws into such a target once its `8_8_8_8` `SNORM` target is drawn (D866). Widening at
the attachment's edges, where every word crosses, keeps the host's word-a-texel handling of
two-byte targets as it is.

**Rejected:**
- An `R16_SFLOAT` attachment: its texels are two bytes, and every buffer, read and reload sized by
  a texel's bytes would carry half the words the host's two-byte handling gives it.
- Converting in the host's texel load and store: those take only the component order, which is
  carried through every frame and band structure that reaches them.
