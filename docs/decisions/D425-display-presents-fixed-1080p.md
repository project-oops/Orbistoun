# D425 - The display presents a fixed 1080p resolution

**Status:** assumed
**Date:** 2026-08-31

`sceVideoOutGetResolutionStatus` writes width and height as `1920`/`1080` at the structure's two
documented leading fields. No other field is written.

**Why:** Every output this project's reference renders at is 1080p, and a hardware run brings up
a matching framebuffer, so the value is measured even though the surrounding layout is not. The
rest of the resolution-status layout has no citable source here.

**Rejected:** leaving the call unimplemented until a variable-resolution model exists - a title
that reads this to size a render target needs an answer now, and a wrong guess at the untested
fields costs nothing a real guest checks.
