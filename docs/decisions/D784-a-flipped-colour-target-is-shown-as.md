# D784 - A flipped colour target is shown as drawn

**Status:** decided
**Date:** 2026-10-09
**known_by:** assumed (what the display reads for a video-out pixel format other than the SDK's is
unmeasured; a title that works on a console shows the colours it drew - D708)

A flipped buffer that is exactly the frame's colour target - its address and extent, untiled
video-out tiling 0 - is shown from the frame's device copy, whatever pixel format the title gave
`sceVideoOutSetBufferAttribute2`. The device copy is linear `Rgba8`, the colours the draws wrote,
and it is shown as-is.

A flipped buffer read from memory instead - one the frame's draws did not target, such as one a
copy filled - decodes by its format's byte order: the SDK's `0x8000000000000000` as B, G, R, A, and
`0x8000000022000000` as R, G, B, A, the order PPSA28061's own draws write it in. Any other format
is not shown.

**Why:** PPSA28061 flips 3840x2160 buffers in format `0x8000000022000000`, not the
`0x8000000000000000` the open-toolchain SDK registers, so no frame of it was shown. Its colour
target is `8_8_8_8` sRGB in the standard order, and the title shows correct colours on a console,
so the display reads the buffer in the order the title drew it. For the SDK's targets, written in
the alternate order and scanned out as B, G, R, A, showing the device copy as-is is exactly what
the old conversion produced, so nothing it showed changes.
