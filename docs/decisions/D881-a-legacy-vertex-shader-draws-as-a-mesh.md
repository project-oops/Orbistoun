# D881 - A legacy vertex shader draws as a mesh shader that assembles its own primitives

**Status:** decided
**Date:** 2026-10-14

A draw whose `VGT_SHADER_STAGES_EN` sets no `PRIMGEN_EN` and no stage before the vertex shader runs
a legacy vertex shader. Its module is still made at the mesh stage (D688), with these differences.

- **Program and registers.** The program is the one at `SPI_SHADER_PGM_LO/HI_VS`, chosen over the
  primitive shader's `_ES` pair for this draw only. Its user data is `SPI_SHADER_USER_DATA_VS_*`
  from `s0`. Its resource words are `RSRC1/2_VS`. Its wave width is `VS_W32_EN`.
- **What the module does itself.** The program declares no counts and exports no primitive, so the
  module does both from the draw:
  - its vertices, a lane each;
  - the primitives its input topology assembles, written as an `exp prim` would write them.
- **Inputs.** The vertex id goes in `v0` (the fetched index, for an indexed draw) and the instance
  id in `v3`. That is radeonsi's GFX10 layout (`declare_vs_input_vgprs`).
- **Output primitive.** It is the input topology's: radeonsi writes `VGT_GS_OUT_PRIM_TYPE` only for
  a primitive or geometry shader (`si_state_draw.cpp` 1083).
- **Streamout stores.** A legacy vertex shader that takes streamout words (`RSRC2_VS.SO_EN`) stores
  its streamout itself. The draw's module drops those stores:
  - the streamout pass carries them out exactly (D861);
  - with streamout off, the hardware hands the wave no vertices to store.

A stream that never wrote `VGT_SHADER_STAGES_EN` is not taken for a legacy draw.

**Why:** radeonsi turns NGG off on GFX10 for every program with transform-feedback outputs, whether
or not feedback is active at the draw. Such a program was bound to no vertex shader at all, and
the draw ran whatever was bound before it. GCTS00001's `KHR-GL30.transform_feedback` group stopped
at its first draw.

**Rejected:**
- **A Vulkan vertex stage for legacy shaders.** Every translated vertex program is a mesh module
  (D688), and the outputs, clip distances, layers and point sizes are written there. A second host
  stage would restate all of it.
- **Running the stores in the draw.** A draw's module does not write guest memory back exactly.
  The streamout pass already does.
