# D870 - A two-channel 32_32 integer target is drawn through R32G32_UINT

**Status:** decided
**Date:** 2026-10-14

A colour target whose `CB_COLOR0_INFO` names `COLOR_32_32`, `NUMBER_UINT` and `SWAP_STD` holds two
32-bit unsigned integers a texel, red then green (`XY__`). It lies by the eight-byte layout, as a
`16_16_16_16` target does (D845), and is drawn through `R32G32_UINT`: its words move in and out as
they are held, a pixel shader's output into it is its integers' raw bits (D851), it is not blended
(D852), and draws write red and green alone.

An element's size is a property of its format, one rule placing every target by it (D869):
`COLOR_32_32`'s is eight bytes.

**Why:** `COLOR_32_32` is 11 and `NUMBER_UINT` 4 (`gfx103.json`). Grand Theft Auto V's second
submission draws into such a target once its `8_8` target is drawn (D869).
