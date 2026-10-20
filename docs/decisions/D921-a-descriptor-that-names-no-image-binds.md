# D921 - A descriptor that names no image binds the null texture

**Status:** decided
**Date:** 2026-10-20

A pixel shader may load an image descriptor from its table whose base address is in no guest
memory. That slot binds the null texture, which reads zero, and does not leave the draw unbound.
A sample through such a descriptor would fault on hardware. The title runs there, so the program
does not reach that sample in this draw. This is `assumed`: no measurement shows which branch a
draw takes, only that a working title cannot take the one that samples.

The rule covers table slots only. A slot read from user data, a compared slot and a volume slot
are refused as before (D743, D817, D823). A descriptor whose image is in guest memory but cannot
be read exactly also stays unbound, so the refusal still names a texture orbistoun reads wrong.

**Why:** Grand Theft Auto V's loading-screen lighting draws use one uber-shader that loads eleven
image slots from its table. Each draw fills the table for the branches it takes, and the other
offsets hold sampler or buffer descriptors. Read as images, those name addresses such as 0x9200
and 0x40c0bfe000. Twenty slots were refused, and the segment, with every submission behind it,
never ran.

**Rejected:**
- Testing for a reserved `TYPE` (0 to 7, `SQ_RSRC_IMG_RSVD_*`, Mesa `gfx6.json:577-584`) instead.
  Some of these words have type 8 (`SQ_RSRC_IMG_1D`), and the type alone does not show that a
  sample cannot happen. An address in no memory does.
- Following the program's branches to the slots one draw reaches. The branch conditions are
  constants in a buffer the draw binds, and nothing here evaluates them before the draw.
