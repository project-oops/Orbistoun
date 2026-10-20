# D916 - A dispatch reads 16-bit standard-swizzle arrays

**Status:** decided
**Date:** 2026-10-20

A dispatch reads the image formats and layouts Grand Theft Auto V's loading-screen programs bind:

- `16_UNORM` (format 7) is one normalised channel in half a word, read through `X001` selects as
  the host's one-channel view answers them (Vulkan `R16_UNORM`).
- `32_32_UINT` (format 62) is two unsigned words, read through `XY01` selects (Vulkan
  `R32G32_UINT`).
- `4KB_S` at two bytes a texel lies by addrlib's equation. Within 64 x 32-texel blocks of 4 KiB,
  x goes to byte bits `0x2 0x4 0x8 0x80 0x200 0x800` and y to `0x10 0x20 0x40 0x100 0x400`
  (`tools/addrlib/equation 4KB_S 2`).
- A `4KB_S` or `64KB_S` array's layers lie a slice of blocks apart with no XOR: addrlib gives a
  standard swizzle no layer terms (`tools/addrlib/equation 4KB_S 2 32 array`, `64KB_S 4 32
  array`). Only the 64 KiB `_X` modes' slices carry the D897 XOR.

A dispatch's fetched images are typed slot by slot, each integer or float as its own format is. A
program fetching from both kinds gets one module that declares each slot as its own (the stored
images stay typed together, D893).

**Why:** Grand Theft Auto V's loading-screen programs fetch a 64 x 64 x 32 `16_UNORM` array in
`4KB_S` beside float images, and store to a 960 x 540 `32_32_UINT` image. Each was refused.
