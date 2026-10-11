# D797 - A quad strip is seeded as the triangle strip over its quads

**Status:** decided
**Date:** 2026-10-11

A draw whose input primitive is `DI_PT_QUADSTRIP` (20, `gfx10.json:811`) seeds its primitive
shader as the triangle strip over the same vertices, cut to its whole quads: an even count of
vertices `n` is `n - 2` triangles, and an odd last vertex, which closes no quad, makes none. The
provoking convention is the triangle strip's, from `PA_SU_SC_MODE_CNTL`.

A quad strip covers the quads `(2i, 2i+1, 2i+3, 2i+2)`, and the triangle strip over the same
vertices covers each of them with two triangles of the same winding, so the two cover the same
pixels. radeonsi hands a GL quad strip to the hardware as this primitive
(`si_state_draw.cpp:630`), and the primitive shader ACO compiles for it takes triangles.

Assumed (`known_by = assumed`): that the geometry engine splits each quad along the diagonal the
triangle strip does, and provokes from the strip's vertex. Either differing changes only flat-shaded
colour and how attributes interpolate across a quad, not which pixels it covers.

**Why:** mesa-demos draws with `GL_QUAD_STRIP`, and its vertex shader was refused at the first draw.

**Rejected:**
- Seeding fans, quad lists and polygons the same way: their vertices are not shared as a strip's
  are, so a strip over them would cover other pixels. They stay refused by name.
