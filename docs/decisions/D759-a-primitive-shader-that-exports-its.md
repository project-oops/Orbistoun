# D759 - A primitive shader that exports its entry v0 runs passthrough

**Status:** decided
**Date:** 2026-10-08
**known_by:** assumed

A primitive shader whose `exp prim` exports the `v0` it was entered with is seeded as a passthrough
primitive shader: `v0` holds its primitive packed as the export takes it, three nine-bit vertex
indices at bits 0, 10 and 20. This holds even when the stream never sets
`VGT_SHADER_STAGES_EN.PRIMGEN_PASSTHRU_EN`. The program decides it. Nothing before the export may
name `v0` as its first operand, which counts as a write.

**Why:** Mesa's NGG lowering hands the export the packed primitive unchanged only in passthrough
mode. Otherwise it packs the export from the vertex indices, which entry `v0` then holds sixteen bits
each (`ac_nir_lower_ngg.c:127-131`, `:139-151`). A shader exporting entry `v0` was compiled for
passthrough, because without it that value is not an export's primitive. PPSA28061's stream never
writes `VGT_SHADER_STAGES_EN`. On hardware, the AGC record its shader goes through carries neither
it nor `GE_CNTL` (REQ-20261008T0130Z-cn06, `reports/hardware/20261008-111104-eboot.obs.log`,
`rec-has-0x2d5 0x0`). Seeded the other way, each triangle's indices were read from the wrong bit
fields, and its text drew as wedges joining one glyph to another.

**Rejected:**
- Waiting for the register's source on hardware (REQ-20261008T1130Z-cn07 (4)): the program already
  says how it was compiled.
- Treating every primitive shader as passthrough: radeonsi's non-passthrough shaders pack their
  own export from the sixteen-bit indices.
