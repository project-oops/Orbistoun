# D742 - An attribute read both interpolated and flat takes a flat twin

**Status:** decided
**Date:** 2026-10-05

The hardware chooses interpolation per instruction: `v_interp_p1_f32` and `v_interp_p2_f32`
interpolate an attribute's channel, `v_interp_mov_f32` reads its provoking vertex's value. A host
fragment input chooses once, by its `Flat` decoration. Mesa reads one attribute both ways: it packs
a varying that is the same at every vertex of a primitive into whichever slot has room, beside
interpolated ones, and reads it flat (`nir_opt_varyings.c`, the convergent groups).

Such an attribute keeps its own location, interpolated, and gets a second, flat location - its
twin - which its flat reads take. The primitive shader exports the parameter at both. The
pipeline gives the twins per draw, from the pair of programs the draw runs: one location each,
from the first above every parameter the primitive shader exports and every attribute the pixel
shader reads, so neither stage's own locations move. Both modules are then translated for that
draw's twins; a pixel shader without them is refused by name. At most eight attributes take a
twin.

**Rejected:**
- Interpolating the flat reads: equal for a value that really is the same at every vertex, up to
  rounding, and wrong for one that is not, which the translation cannot tell apart.
- A fixed offset for the twins' locations: past the locations a device is guaranteed.
- Per-channel inputs sharing the location with different decorations: interface matching
  requires one interpolation per location.
