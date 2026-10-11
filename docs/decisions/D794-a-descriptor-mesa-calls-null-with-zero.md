# D794 - A descriptor Mesa calls null with zero selects samples zero

**Status:** decided
**Date:** 2026-10-11

An image descriptor whose second word is zero and whose `DST_SEL_X` .. `DST_SEL_W` are all
`SQ_SEL_0` binds as the null texture: one texel, zero in every channel. Whatever its other words
hold, it is read neither as an address nor as a format.

Two independent groundings agree:
- Mesa treats a descriptor whose second word is zero as null, and answers zero for it
  (`ac_nir_lower_resinfo.c:20-24`, `handle_null_desc`). That word holds the format and the top
  of the base address, so such a descriptor names no image.
- Each select set to `SQ_SEL_0` makes a sample return the constant zero in that channel whatever
  the texel (`gfx10-rsrc.json`, `SQ_IMG_RSRC_WORD3`).

An all-zero descriptor, the null texture before this decision (RADV's `memset(dst, 0, size)`),
is one case of this rule.

Assumed (`known_by = assumed`): that the hardware fetches nothing it could fault on for such a
descriptor. A descriptor with a zero second word and any select other than `SQ_SEL_0` stays
unbound and refused by name.

**Why:** PPSA28061, run on the host clock, blends two textures as `B + s0 * clamp(A - B)` with
`s0 = 0`. Texture A's eight user-data words are `0, 0, 0x800eb0, 0x6000, ...`: Mesa's null, with
every select zero. The draw was refused, which stopped the submission and the ten behind it.

**Rejected:**
- Binding any descriptor whose selects are all constants: a descriptor that names an image would
  then rest on the selects alone, a reading no driver documents.
