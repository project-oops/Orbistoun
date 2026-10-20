# D927 - A stencil plane a draw loads binds as its values' bits

**Status:** decided
**Date:** 2026-10-20

A pixel shader may load the stencil plane of a depth target as a texture. Its descriptor is
`8_UINT` under a depth swizzle, flat, at the base the submission's stencil tests read or write
(`DB_STENCIL_READ_BASE`, `DB_STENCIL_WRITE_BASE`). That slot binds the target's stencil as the host
attachment holds it. Each value is widened to a word and fetched through an `R32_SFLOAT` view.

A load of an `8_UINT` texel answers the value as an unsigned integer in the register. A fetch from a
32-bit float format neither filters nor converts, so it hands those same bits to the module. The
module declares a float image, as every draw module does, and needs no integer image type and no
second translation.

This is `assumed` for a filtered sample, where the hardware's answer for an integer format is
not established. Only an unfiltered load is grounded. A slot compared, sampled as a cube or a
volume, or loaded by layer is refused as before.

**Why:** Grand Theft Auto V's loading-screen lighting tests the scene's stencil and also loads it
through its descriptor table at the stencil read base. That one descriptor was the last of the
segment's unbound slots, and the segment, with every submission behind it, never ran.

**Rejected:**
- Declaring an unsigned integer image in the module and translating it again for the draws whose
  slot binds one. A dispatch does this (D916), but a draw's module is shared by draws whose
  tables differ, and one uber-shader would be translated per table.
- An `R8_UINT` view. Its fetch is an unsigned integer, which a float image type cannot declare.
