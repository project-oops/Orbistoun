# D869 - A two-channel 8_8 target is drawn through an Rgba8 attachment

**Status:** decided
**Date:** 2026-10-14

A colour target whose `CB_COLOR0_INFO` names `COLOR_8_8`, `NUMBER_UNORM` and `SWAP_STD` holds two
bytes a texel, red then green (`ac_formats.c` `ac_translate_colorswap`, `XY__`). It lies by the
two-byte layout, as the other two-byte targets do (D852, D868), a host carrying each texel's two
bytes in a word's low half. A drawer draws it through `R8G8B8A8_UNORM`: the two bytes enter with
blue zero and alpha one - what a format without them reads as - and leave as the two bytes alone.
Draws write red and green only, so blending that reads destination alpha reads one.

**Why:** `COLOR_8_8` is 3 (`gfx103.json`, enum `ColorFormat`). Grand Theft Auto V's second
submission draws into such a target once its half-float target is drawn (D868). The word-a-texel
handling of two-byte targets and the widening at the attachment's edges are D868's.

**Rejected:**
- An `R8G8_UNORM` attachment: its texels are two bytes, against the four every buffer sized by a
  texel's bytes carries for a two-byte target.
