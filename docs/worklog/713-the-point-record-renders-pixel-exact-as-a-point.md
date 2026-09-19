# 713. The point record renders pixel-exact as a point, and the assumption is now measured

**2026-09-19** — inbox `-0c58`, the acceptance closer: worklog 712 made the mesh output
topology-specific but left the point record's render "not yet verified" and one thing assumed - that
a point packs its `exp prim` index in the same low field a triangle uses. Both are now settled by a
render on hardware.

## The experiment the near-identical streams made possible

The point record (`agc-primitive-draw-fw1240`) and the triangle record (`-triangle-`) are twins:
their command streams differ only in `VGT_GS_OUT_PRIM_TYPE` (0 = POINTLIST against 2 = TRISTRIP) and
two counts, and they name the **same** shader addresses. The point record did not capture its own
shaders, but that near-identity is the evidence they share a primitive program - so the triangle's
captured shaders can drive the point stream, and the only thing that changes the picture is the
topology this crate now threads into the mesh output.

`a_point_draw_renders_as_a_point_not_a_triangle` (in `console_triangle.rs`, beside f50b) does exactly
that: it walks the point stream with the triangle's shaders, and:

- **Device-free and certain:** the topology decodes to a point list through the whole submit path.
- **On a device (NVIDIA RTX 5070 Ti):** both shaders translate, the point-shaped mesh module is one
  the driver accepts (the new `OutputPoints` + `PrimitivePointIndicesEXT` + scalar-index SPIR-V is
  valid), the draw runs, and it lights **one** texel - not the triangle's 512. It is not a triangle
  list (acceptance 2/4).
- **Pixel-exact:** the console lit texel `(15, 15)`; orbistoun lit `(15, 15)`. The same point, in
  the same place.

## What the match settles

- **The assumption is now a measurement.** 712 read the point's one index from `exp prim` field 0 on
  the assumption a point packs like a triangle's first index; the point landing on the console's
  exact texel confirms that field 0 is the right one. It is no longer assumed - it is measured by the
  render, the strongest oracle this project has (framebuffer diffing, principle "where the oracle
  comes from" §2).
- **The two records share a primitive program.** The triangle's shaders producing the console's exact
  point is that shown, not argued - a different program would not have landed on `(15, 15)`.
- **0c58 is closed.** Acceptance (1) the topology is decoded and carried (702); (2) a device builds a
  point pipeline that is not a triangle list (this); (3) an unmapped topology is refused by name
  (712's unit test); (4) no fixed topology remains on the draw path bar the inert input-assembly one
  a mesh pipeline ignores. And the point record renders pixel-exact, which the acceptance's spirit
  wanted and its letter did not quite reach.

## What is still true and unclosed elsewhere

The line path (`OutputLinesEXT`, `uvec2` indices) is built and unit-covered but has no measured
record to render against - no line draw was captured. It rests on the same packing the point
confirmed, one field wider; a line record would measure it as this measured the point. Recorded, not
claimed.

## Gate state

`./bin/orbistoun check` passes end-to-end (all checks passed): the whole workspace compiles, clippy
`-D warnings` clean, the device triangle **and** point renders pass, docs/prose/fmt clean, identity
scan clean. No commit.
