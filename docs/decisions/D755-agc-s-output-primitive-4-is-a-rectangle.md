# D755 - AGC's output primitive 4 is a rectangle list

**Status:** decided
**Date:** 2026-10-07

`VGT_GS_OUT_PRIM_TYPE` 4 is read as a rectangle list, as 3 already is. Each primitive's three
corners and the fourth the rasteriser completes are assembled as two triangles (`MeshPrimitive::
Rectangles`). Measured on a draw since (obSCEne, `reports/hardware/20261007-202010-eboot.obs.log`
lines 8223-8236, check `166-agc/primitive-draw-rectlist`): a draw of topology 17, whose prim state
carries 4, fills a rectangle, and so does one of topology 7, whose prim state carries 3. A
back-facing topology-17 rectangle is culled when back faces are; a topology-7 one is drawn either
way, which orbistoun does not distinguish yet.

**Why:** `sceAgcCreatePrimState` writes the output primitive into its register list, and
`166-agc/create-prim-state` measured it for eight input topologies: 2 for the triangle topologies,
3 for a quad list (7), and 4 for a rectangle list (17, `DI_PT_RECTLIST`). That is gfx11's numbering
(`gfx11.json` and `gfx12.json`: `RECT_2D` 3, `RECTLIST` 4), not gfx103's, whose enum stops at
`RECTLIST` 3. radeonsi built for this console writes 3 for its rectangle blits, and those draw on
hardware in every oops-apps title. So both values are rectangles here. PPSA03416 clears with a
rectangle list through the prim state.

**Rejected:**
- Refusing 4 as unknown. It holds PPSA03416's first draw, and the value is the library's own for a
  rectangle list.
- Reading 3 as gfx11's `RECT_2D`, a different shape. radeonsi's 3 is drawn as a rectangle list on the
  console, and a quad list arriving as 3 is drawn as rectangles until a title shows otherwise.
