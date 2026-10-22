# D942 - A texture whose keys are all uncompressed is read as plain texels

**Status:** decided
**Date:** 2026-10-21
**known_by:** published

A texture descriptor may enable delta colour compression (`COMPRESSION_EN`, dword 6 bit 21) over a
surface whose DCC keys all read `0xFF`, every block stored uncompressed. Memory then holds the plain
surface, and the texture is read as if compression were off. This is the state a colour target is
left in once a frame is written back into it, its keys expanded (D714). It holds for one level of
`64KB_R_X`, whose key layout is modelled; a chain, another layout, or keys that say anything else
is read as before.

**Why:** Grand Theft Auto V samples its 3840x2160 `16_16_16_16_FLOAT` scene target through a
compressed descriptor after drawing into it, and every texture reader refused a compressed image.
