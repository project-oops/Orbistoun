# D830 - User clip planes are clip distances from specialization constants

**Status:** decided
**Date:** 2026-10-14

Every translated mesh module carries six clip distances in its per-vertex block. Each vertex writes
`dot(plane_k, position)` against its clip-space position. The planes are 24 `float`
specialization constants (`SpecId` `4 * plane + axis`), and their default of zero clips nothing.
The rectangle-list epilogue writes its fourth corner's distances the same way.

The pipeline reads `PA_CL_CLIP_CNTL`'s `UCP_ENA_0` to `_5` and `PA_CL_UCP_*` at each draw. It
sends the enabled planes as `SetClipPlanes`, with any disabled plane as zero, and sends nothing
when `CLIP_DISABLE` is set. The Vulkan backend specialises the mesh stage with the planes, and
the planes join the pipeline key.

**Why:** the guest's clipper cuts geometry against these planes, and Vulkan's clip distances do
the same: a primitive is clipped where a distance is negative. oops-gl writes its GL clip planes
there in clip space (`gl_draw.c:645-678`). Neverball uses them for its floor reflection and to
split its ball. Specialization keeps the planes out of the per-draw words, and a module whose
planes are all zero draws as before.

**Rejected:**
- Discarding fragments from a reconstructed clip position: it needs the inverse viewport in every
  fragment module, and it is exact only where the rasterised depth is.
- Declaring clip distances only in modules translated for a draw with planes: translation is
  shared across draws, so one module would need a variant for each plane set.
- Serving `UCP_CULL_ONLY_ENA` separately: it is not distinguished here; a cull-only plane is
  clipped, which differs only for a primitive that crosses it.
