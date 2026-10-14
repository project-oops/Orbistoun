# D867 - A vertex's exported point size is the size it is drawn at

**Status:** decided
**Date:** 2026-10-14

A primitive shader's position exports after `pos0` are routed by the draw's `PA_CL_VS_OUT_CNTL`,
carried into the translation as `UserData::position_vectors` and keyed into the module.

**Routing.** The enabled vectors follow `pos0` in a fixed order:

1. the misc vector, where `VS_OUT_MISC_VEC_ENA` is set;
2. distances 0-3, where `VS_OUT_CCDIST0_VEC_ENA` is set;
3. distances 4-7, where `VS_OUT_CCDIST1_VEC_ENA` is set.

So `pos1` is the misc vector only when the draw exports one; otherwise it carries the first
distances. This is radeonsi's `ac_nir_prerast_utils.c` 355-430 and `si_state_shaders.cpp`. A vector
the draw does not enable reaches nothing. With no routing given, `pos1` is the misc vector, as D859
took it, and `pos2` and `pos3` are refused.

**The misc vector:**

| Channel | Carries | Handling |
|---|---|---|
| X | point size | Written to the vertex's `PointSize` where `USE_VTX_POINT_SIZE` has the hardware read it. radeonsi sets that exactly when the shader writes one. A vertex that exports none is drawn at the draw's size, below. |
| Y | edge flag | Dropped. It selects the edges a polygon drawn as lines draws, and every polygon here is filled. The edge flags in a primitive export are dropped for the same reason. |
| Z | layer | Handled as D859. |
| W | shading rate | Dropped where `USE_VTX_VRS_RATE` is clear, refused where it is set. |

**A point that exports no size** is drawn at the draw's `PA_SU_POINT_SIZE`: its height in eighths
of a pixel, as radeonsi writes it (`si_state.c` 945: `size * 8` in both the height and the width).
Where that is not known, the point is drawn at one. Every module that emits points therefore writes
`PointSize`, since a mesh stage's point has no size of its own.

**Distances.** A distance whose `CLIP_DIST_ENA_n` is set is written to the vertex's `ClipDistance`
in its index's place. It stands in for the user plane's computed distance there (D830), and the
array grows to eight. radeonsi also sets `CULL_DIST_ENA_n` for every distance it clips by
(`si_emit_clip_regs`: `culldist_mask |= clipdist_mask`). Culling by a distance that also clips
removes nothing the clip keeps, so such a distance is a clip distance. A distance that only culls is
refused. One the draw neither clips nor culls by reaches nothing.

**Why:** GCTS00001's `KHR-GL30.clip_distance.functional` exports one, two, then four clip
distances through `pos1`, with no misc vector. Taking `pos1` for the misc vector would have read
those distances as a point size, an edge flag and a layer.

**Rejected:**
- **Inferring the routing from the shader alone.** One `exp pos1` with its first channel is a point
  size under one draw and clip distance zero under another. Only the register tells them apart.
