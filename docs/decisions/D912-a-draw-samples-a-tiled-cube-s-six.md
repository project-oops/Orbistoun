# D912 - A draw samples a tiled cube's six faces

**Status:** decided
**Date:** 2026-10-20

A draw's cube texture in a tiled layout binds its six faces' first level. Face F is placed as an
array's layer F (D910): F chains on from the base, with the layer's pipe-bank XOR in a 64 KiB
`_X` mode. Each face is read in a layout a dispatch reads. A view that does not start at the
first face and level, is not one cube's six faces, or is compressed, is not bound.

The backend also binds a guest's mip chain for any flat image of one word a texel, not only the
eight-bit ones (D833): packed floats and `2_10_10_10` are staged level after level in their own
format.

**Why:** Grand Theft Auto V samples the 1024 x 1024 `10_11_11_FLOAT` cube in `64KB_S` that its
compute dispatches stored. A chained `10_11_11_FLOAT` texture was then refused at the device.
