# D798 - A quad list is seeded two triangles a quad

**Status:** decided
**Date:** 2026-10-11

A draw whose input primitive is `DI_PT_QUADLIST` (19, `gfx10.json:810`) seeds its primitive shader
two triangles for each whole quad of four vertices, split as Mesa splits a quad for its provoking
convention (`u_indices_gen.py:129-136`): `(v0, v1, v3)` and `(v1, v2, v3)` when the last vertex
provokes, `(v0, v1, v2)` and `(v0, v2, v3)` when the first does, the convention read from
`PA_SU_SC_MODE_CNTL` as a strip's is (D730). Vertices past the last whole quad make nothing.

Assumed (`known_by = assumed`): that the geometry engine splits a quad along the same diagonal.
Another split covers the same pixels and differs only in how attributes interpolate across the
quad. radeonsi hands a GL quad list to the hardware as this primitive (`si_state_draw.cpp`'s
primitive table).

**Why:** mesa-demos draws with `GL_QUADS`, and its vertex shader was refused at the first such draw
once its quad strips (D797) were seeded.

**Rejected:**
- Seeding a quad as a two-triangle strip: the strip's second triangle reverses its winding rule,
  which is right for a strip's shared edge and not for a quad's own vertices.
