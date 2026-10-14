# D844 - A one-channel 32-bit float target is drawn and written back as it is held

**Status:** decided
**Date:** 2026-10-14

A colour target whose `CB_COLOR0_INFO` names `COLOR_32`, `NUMBER_FLOAT` and `SWAP_STD` holds one
32-bit float a texel, red. It is drawn through a Vulkan `R32_SFLOAT` attachment. Its image is
the same format, so seeding it from guest memory and writing it back move the guest's words
untouched, as D773 does for `10_11_11`. Any other swap on `COLOR_32` stays refused.

**Why:** `COLOR_32` is what Mesa targets a one-channel 32-bit format at (`ac_formats.c` 489), and
`SWAP_STD` on one channel is red (892, the `SX_RT_EXPORT_32_R` case). Grand Theft Auto V's second
draw, once its first ran (D843), writes a 5x5 `COLOR_32` `FLOAT` target. That is a reduction
target, and nothing an eight-bit attachment holds can be written back to it exactly.

**Open:** Vulkan does not require blending on `R32_SFLOAT`. The desktop hosts orbistoun runs on
offer it, as they offer it on D773's packed format. A host without it would fail the pipeline
rather than draw without blending.

**Rejected:**
- Drawing it through an eight-bit attachment and converting: a float written back from eight
  bits is not the value the shader wrote.
- `R32_SFLOAT` for the other swaps: `SWAP_ALT_REV` puts the channel in alpha (892), which a
  one-channel attachment does not hold.
