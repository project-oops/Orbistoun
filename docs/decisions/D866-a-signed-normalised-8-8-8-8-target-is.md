# D866 - A signed-normalised 8_8_8_8 target is drawn through its own format

**Status:** decided
**Date:** 2026-10-14

A colour target whose `CB_COLOR0_INFO` names `COLOR_8_8_8_8` and `NUMBER_SNORM`, in a component
order an `8_8_8_8` `UNORM` target may take, holds four signed-normalised bytes a texel. A drawer
draws it through `R8G8B8A8_SNORM`, for the image and the attachment alike, so a draw's output and
blending are signed and the bytes read in and written back are the guest's own, as they are for
the other four-byte targets (D773, D848).

**Why:** `NUMBER_SNORM` is 1 (`gfx103.json`, enum `SurfaceNumber`). Grand Theft Auto V's second
submission draws into a 128 x 128 linear target of it, once its first submission completes (D823).

**Rejected:**
- Drawing it through the `UNORM` attachment and reinterpreting: a negative output would clamp to
  zero, and blending would read the bytes as unsigned.
