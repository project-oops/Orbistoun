# D106 - Auto fidelity reads the shader and warns

**Status:** decided
**Date:** 2026-08-20

The lane model refuses to write the execution mask. `Fidelity::Auto` inspects a shader's
operands, chooses the wavefront model when the shader touches the mask, and reports the choice
as a warning carrying the subgroup width that would serve it faster.

**Why:** a model with no inactive lanes runs every lane regardless, which is plausible and
wrong. Refusing whenever a shader masks would refuse nearly every real shader. The fallback
costs a factor of the wavefront width, so it is a warning a caller must decide to ignore, not a
field a caller must know to read.

**Rejected:**
- Ignoring mask writes in the lane model: silently wrong.
- Choosing by opcode: forces every 64-bit move onto the slow model.
- Choosing the subgroup level automatically: the translator never sees the device.
