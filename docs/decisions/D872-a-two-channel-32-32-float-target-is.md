# D872 - A two-channel 32_32 float target is drawn through R32G32_SFLOAT

**Status:** decided
**Date:** 2026-10-14

A colour target whose `CB_COLOR0_INFO` names `COLOR_32_32`, `NUMBER_FLOAT` and `SWAP_STD` holds two
32-bit floats a texel, red then green (`XY__`). It lies by the eight-byte layout, as its `UINT` twin
does (D870), and is drawn through `R32G32_SFLOAT`, its words moved in and out as they are held and
red and green written.

**Why:** `COLOR_32_32` is 11 and `NUMBER_FLOAT` 7 (`gfx103.json`). Grand Theft Auto V's nineteenth
submission draws into such a target once its 16-bit packs translate.
