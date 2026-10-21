# D936 - A layered depth-only draw is drawn a pass per depth slice

**Status:** decided
**Date:** 2026-10-21
**known_by:** published

A draw may have the hardware read each primitive's layer (`PA_CL_VS_OUT_CNTL.USE_VTX_RENDER_TARGET_INDX`)
while no colour buffer is bound (`CB_COLOR0_INFO` `COLOR_INVALID`, D779). The layer then indexes
the depth view's slices, `DB_DEPTH_VIEW.SLICE_START` to `SLICE_MAX` (`gfx103.json`: bits 10:0
with 12:11 above them, and 23:13 with 31:30). Where that view spans more than one slice, the draw
runs once per slice. Each pass binds that slice's depth target, as a cube's faces are bound
(D918), and keeps the primitives whose layer is its own, counted from zero (D859). After the last
pass the submission's own targets are bound again.

D859 read a draw's slices from `CB_COLOR0_VIEW` alone. For a depth-only draw that view is unset,
so it ran the draw once, keeping layer zero, and every primitive aimed at another slice was
culled.

**Why:** Grand Theft Auto V draws each point light's shadow cube in one depth-only draw, and its
primitive shader sends each triangle to its face. Only face 0 was drawn. The lighting pass then
sampled faces 1 to 5, which nothing had drawn, and the backend refused the segment.

**Rejected:**
- An array depth attachment with the host's `Layer` output: every depth target, pipeline and
  depth read-back would carry a layer count only layered draws use, as D859 already found for
  colour.
