# D928 - Tessellated draws run their hull as compute and tessellate on the CPU

**Status:** decided
**Date:** 2026-10-20

A tessellated draw is carried out stage by stage, as the hardware runs it. It is not translated
into the host's tessellation stages.

1. **Local and hull shader as compute.** The program at `SPI_SHADER_PGM_LO/HI_HS` is the LS and
   HS merged (Mesa `si_state_draw.cpp:104-131`). It runs as exact compute waves over the draw's
   patches, the route D861 takes for a streamout vertex shader. It stores each patch's
   tessellation factors to the TF ring and its outputs to the off-chip buffer itself, with the
   `buffer_store`s it was compiled with (`ac_nir_lower_tess_io_to_mem.c:1126-1165`,
   `:1403-1466`). Both are in guest memory, so a title that reads them back reads what the
   console would hold.
2. **The tessellator on the CPU.** `orbistoun-gpu::tessellation::tessellator` reads each patch's
   factors out of the ring and makes its domain points and primitives. It is a port of the D3D11
   functional specification's reference tessellator (`CHWTessellator`, Microsoft, MIT, carried
   in Mesa at `src/gallium/auxiliary/tessellator/tessellator.cpp`). It keeps that code's 16.16
   fixed point and its order of points and indices. The `CHLSLTessellator` half is not ported:
   it is code the HLSL compiler puts in a hull shader, which a console hull shader carries in
   its own program. The port is an exception to ACKNOWLEDGEMENTS' "reference only, never
   copied", as D926's is. The reference states what hardware must produce, and its header says
   implementers "need only look at this class". Rewriting it from a prose description would have
   to reproduce it bit for bit anyway, or be wrong. The licence's notice is in
   `crates/orbistoun-gpu/LICENSE-D3D11-tessellator`.
3. **The domain shader through the mesh path.** The program in the ES or VS slot (`ES_EN` or
   `VS_EN` = `*_STAGE_DS`) runs once per domain point through the primitive-shader route that
   D688 and D881 already take. Its `u, v`, relative patch id and patch id come from the
   tessellator's output in the VGPRs the census measures. Its primitives are the tessellator's
   indices.

Until all three are wired, a draw whose `VGT_SHADER_STAGES_EN` enables any tessellation stage
(`LS_EN`, `HS_EN`, or a domain shader in the ES or VS slot) is refused by name. The refusal names
its domain, partitioning, output and control points (`tessellation::refusal`), and it is counted
in the submission report. Without the refusal, the domain shader would be drawn as if it were the
vertex stage.

**Decoded from published sources, measured before relied on.** The register fields come from
`gfx103.json`. The TF ring's layout is radeonsi's: outer before inner, per patch at the
relative patch id times the domain's dword count, and isoline factors stored with line detail
first. The off-chip parameter's fields come from `ac_gpu_info.c:1413-1415`. Each of these, plus
what `sceAgcDriverSetHsOffchipParam` and `sceAgcDriverSetTFRing` write, the registers the AGC
builders set for an LS/HS pipeline, the stages' input SGPRs and VGPRs, and the tessellator's
output per partitioning, is a row of the obSCEne tessellation census. The two driver calls keep
their measured "answer 0, write nothing" until the census shows otherwise.

**Why:** a tessellated draw's intermediate results live in guest memory on this hardware, and
the hull shader is written to the hardware's contract, not to a host API's:

- it stores factors and control points with buffer instructions at offsets the hardware hands
  it;
- the LS hands the HS its outputs through LDS laid out by the guest's compiler.

Running that program as compute keeps the TF ring and the off-chip buffer byte for byte, takes
the existing exact-compute route (D705, D746, D861), and needs nothing new from the translator.
Running the tessellator on the CPU makes its output a pure function of the factors, testable
without a GPU and comparable point for point with the census. The cost is a read-back of the
ring between the hull and domain stages. A compute pass can take the tessellator over later
without changing the contract, if that read-back becomes the cost of a frame.

**Rejected:**
- Translating the LS and HS into a Vulkan tessellation-control shader and the DS into a
  tessellation-evaluation shader, with LDS and off-chip I/O mapped to patch variables. The
  guest's stores to the ring and the off-chip buffer would have to be pattern-matched back into
  `gl_TessLevel*` and patch outputs, which only a compiler's own idioms make recognisable.
  Guest memory would hold nothing where the console holds the factors. The host's tessellator
  is also not required to match the console's: Vulkan leaves the fractional spacings' exact
  positions to the implementation.
- Tessellating with a compute shader now. It is the same contract run on the device, and only
  worth having once the CPU version is measured against the console and found slow.
