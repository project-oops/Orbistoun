# D670 - Unimplemented calls answer a negative placeholder

**Status:** decided
**Date:** 2026-09-10

An unimplemented call answers the placeholder base `0xF7FF_0000` plus its reason code. The value
is negative, so a guest's own `rc < 0` check catches it, and it lies outside the `0x80xx_xxxx`
range every measured platform error uses; fault lookup also recognises its sign-extended 64-bit
form.

**Why:** every error the platform has been measured returning sets the high bit, so guests test
the sign. A positive placeholder reads as success to a guest that checks correctly, and a guest
that believed one built pointers from what followed; with the high bit set, guests name the
failing call themselves. Keeping clear of `0x80` stops the placeholder being read as firmware
behaviour, and keeping the low half lets a reader recognise it on sight.

**Rejected:**
- `0x7FFF_0000`: positive, so a guest reads a refusal as success.
- A `0x80xx_xxxx` code: indistinguishable from a measured platform error.
- Answering success: plausible output no guest can detect.
