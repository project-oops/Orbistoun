# D906 - A one-layer depth array is the held depth target

**Status:** decided
**Date:** 2026-10-19

A dispatch may read a `32_FLOAT` image in a depth swizzle as a 2D array of one layer:
`SQ_IMG_RSRC_WORD3.TYPE` 2D_ARRAY, with `DEPTH` and `BASE_ARRAY` both zero, read by a program that
addresses it as an array. That image is the depth target at its base, which the host holds, as a 2D
image is (D894). Its depths come from the host and are bound as an array of one layer.

An array of more than one layer is not one target, and is read from guest memory as before.

**Why:** Grand Theft Auto V's loading-screen program reads its 1024 x 1024 depth this way. Read
from guest memory, the image's metadata looked like DCC keys of which a quarter were compressed.
They are HTILE, one word per 8 x 8 tile, every word `0xfffffff0`: depth cleared to 1.0
(`radv_cmd_buffer.c:15355-15360`, `ac_descriptors.h:148-154`). Guest memory holds only the clear;
the host holds the depths drawn since.

**Rejected:**
- Filling the image from its HTILE clear value: it would read 1.0 where the host has drawn depth.
- Classifying `64KB_Z_X` metadata as DCC keys. addrlib sizes and addresses a `64KB_Z_X` colour
  surface's keys as `64KB_R_X`'s (`tools/addrlib/dcc` under `DCC_SWIZZLE`), but no title is known
  to need it, and a depth image's metadata is not DCC.
