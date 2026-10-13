# D825 - An export's colour target is a location, bound by SPI_SHADER_COL_FORMAT

**Status:** decided
**Date:** 2026-10-13

A translated `exp mrtK` stores to the fragment module's `vec4` output at `Location K`. Every
fragment module declares location zero; it declares another location only when one of its exports
names that target. This replaces D553's refusal of every target but `mrt0`.

Which colour buffer location `K` is comes from the registers the guest wrote. It is the `K`th
colour buffer whose four-bit `SPI_SHADER_COL_FORMAT` field is not `SPI_SHADER_ZERO`: a buffer with
no format takes no export number. The pipeline lists those buffers in that order as the render
pass's colour attachments. Colour buffer `n`'s registers are read by the decoders written for
colour buffer zero, through a view that moves them onto buffer zero's addresses:

- `CB_COLORn_BASE` to `DCC_BASE` are fifteen dwords on per buffer.
- Each `*_EXT`, `ATTRIB2` and `ATTRIB3` is one dword on.
- `CB_BLENDn_CONTROL` is one dword on.
- `CB_TARGET_MASK` and `CB_SHADER_MASK` are shifted four bits per buffer.

The first attachment keeps D714's write-back, deferred to the flip where it was. Each further
attachment is seeded from guest memory before its submission is drawn and written back after it.

A draw is refused, and counted, when its pixel shader writes channels to a target past the colour
buffers the submission binds.

**Why:** the compaction is published. ACO skips a `ZERO` slot (`aco_select_ps_epilog.cpp:88-91`)
and numbers each export by the exports before it (`:428-447`). The register strides are published
in `gfx103.json`. So which attachment a target selects is no longer the unknown D553 refused it
for. oops-gl's second draw buffer exports `mrt0` and `mrt1` and binds `CB_COLOR1`, and both GL
probes stopped there.

**Rejected:**
- Taking target `K` as colour buffer `K`: right only while no lower buffer is unformatted, so it
  draws a plausible wrong frame the first time one is.
- Writing every attachment back at the flip, as the first is: the deferred write-back holds one
  frame per target (D714, D801), and the further attachments are what a second draw buffer reads
  back within the same frame.
- Declaring all eight outputs in every fragment module: a module would then claim exports it never
  makes.
