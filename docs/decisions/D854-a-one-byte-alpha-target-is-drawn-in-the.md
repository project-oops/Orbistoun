# D854 - A one-byte alpha target is drawn in the frame's alpha and written back as it is held

**Status:** decided
**Date:** 2026-10-14

A colour target whose `CB_COLOR0_INFO` names `COLOR_8`, `NUMBER_UNORM` and `SWAP_ALT_REV` holds
one byte a texel, alpha. It lies by the one-byte layout, as a red `COLOR_8` target does. A
drawer stands an `Rgba8` frame in for it, as for the red one: each byte is read into the frame's
alpha, with the colour channels zero, and written back from the frame's alpha. A draw into it
writes alpha alone, as its write mask already says.

A fill pattern is a uniform clear only when every texel in its word is alike: four bytes at one
byte a texel, two halves at two (D852). A pattern whose texels differ is laid out where the
layout puts them.

**Why:** `SWAP_ALT_REV` on one channel is `___X`, alpha (`ac_formats.c` 586). Grand Theft Auto V
draws into a 1920 x 1080 target of it, once its earlier draws run (D851-D853). The two-byte fill
check fixes D852, which took a word whose halves differ as one value.

**Rejected:**
- Reading the byte into red as for `SWAP_STD`: blending reads destination alpha from the frame,
  and an alpha target's blend would read a constant.
