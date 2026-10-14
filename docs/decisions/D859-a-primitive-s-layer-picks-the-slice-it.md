# D859 - A primitive's layer picks the slice it is drawn into

**Status:** decided
**Date:** 2026-10-14
**known_by:** published

A primitive shader that exports `pos1` with its third channel enabled names the render-target
layer of each vertex there: bits 10:0, with the viewport index in 19:16 (Mesa
`ac_nir_prerast_utils.c` 381-392, for GFX9 and later). Where `PA_CL_VS_OUT_CNTL.USE_VTX_RENDER_TARGET_INDX`
(bit 18) is set, the primitive is drawn into slice `SLICE_START + layer` of each colour target,
`CB_COLORn_VIEW` giving `SLICE_START`; where it is clear, the export is ignored and every
primitive goes to `SLICE_START`. radeonsi and RADV set that bit exactly when the shader writes the
layer (`si_state_shaders.cpp` 996, `radv_shader.c` 1697).

A colour target is the slice `SLICE_START` names: its base moves by that many slices, each slice a
whole mip chain (`SurfaceLayout::chain_bytes`), as a sampled cube's faces lie.

The draw runs once per slice its view spans. Each pass has one slice as its target, and its draw
data carries the layer it draws (`DRAW_DATA_LAYER_WORD`); the mesh module stages each vertex's
layer and culls every primitive whose layer is not that one (`CullPrimitiveEXT`). A draw that
does not use the layer carries `u32::MAX` there and culls nothing.

**Why:** PPSA21564's first draw is a full-screen triangle whose primitive shader exports the
layer from `v8`, the instance id, so an instanced draw fills one slice per instance - the usual
way to clear or copy every slice of an array. Culling against the pass's layer draws each
primitive into the slice its own shader named, whatever it computed, and keeps the
single-slice targets, write-back and pipelines every other draw uses. A primitive's layer is its
first vertex's: GL leaves a primitive whose vertices disagree undefined, and the shaders that
write it give every vertex of a primitive the same one.

**Assumed:**
- A layer past `SLICE_MAX - SLICE_START` draws nothing. What the hardware does with one is not
  measured.
- A primitive's layer is its first vertex's rather than its provoking vertex's. They differ only
  under `PROVOKING_VTX_LAST` with vertices that disagree.

**Rejected:**
- One pass per instance, each into its own slice: right only where the layer is the instance id.
- Array attachments with the host's `Layer` output: every target, write-back and pipeline would
  carry a layer count that only layered draws use.
- The viewport index (`USE_VTX_VIEWPORT_INDX`): a draw that sets it stays refused until a title
  needs more than one viewport.
