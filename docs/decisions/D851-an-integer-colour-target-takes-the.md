# D851 - An integer colour target takes the exported bits as they are

**Status:** decided
**Date:** 2026-10-14

A draw tells the translator which of its pixel shader's colour targets hold integers
(`UserData::integer_targets`, bit `K` for `mrtK`). Such a target's fragment output is a `uvec4`,
not a `vec4`. An export to it stores its sources' bits as they are, not reinterpreted as floats.
A compressed export to it stores its two 16-bit halves of each source as integers, not as
halves. The draw's mask is part of the module's cache key.

**Why:** Vulkan requires a fragment output's component type to match its attachment's: a
`uvec4` for an unsigned-integer attachment. Mesa's ACO exports an integer target's value as
the integer it is, packing two 16-bit integers into a register for a `UINT16_ABGR` export format
(`v_cvt_pk_u16_u32`), where a float target's export is a float or two halves. Grand Theft Auto V
draws into a 3840 x 2160 `COLOR_16` `NUMBER_UINT` target.

**Rejected:**
- Converting each exported float to an integer: an integer target's export holds its integer
  bits, and converting them would change them.
- One output type for every target of a module: a pixel shader may write a float target and an
  integer one in the same draw.
