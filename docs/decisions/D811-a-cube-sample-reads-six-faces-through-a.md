# D811 - a cube sample reads six faces through a cube view

**Status:** decided
**Date:** 2026-10-12

A sampling instruction whose dimensionality is a cube (`dim:SQ_RSRC_IMG_CUBE`, code 3) is
translated. Its address is a face's two coordinates and the face, as ACO forms them for the
hardware: each coordinate `sc / |ma| + 1.5`, the face `cube_amd`'s id
(`ac_nir_lower_tex_coords.c`, `prepare_cube_coords`). The translation rebuilds the direction those
name, inverting `cube_amd` with the major component one half, so `s - 1.5` and `t - 1.5` are the
face's `sc` and `tc`:

| face | direction |
|---|---|
| 0 (+x) | (0.5, -v, -u) |
| 1 (-x) | (-0.5, -v, u) |
| 2 (+y) | (u, 0.5, v) |
| 3 (-y) | (u, -0.5, -v) |
| 4 (+z) | (u, -v, 0.5) |
| 5 (-z) | (-u, -v, -0.5) |

It samples that direction through a binding declared `DimCube`. The host's cube face selection
(the Vulkan specification's cube map face selection table) is `cube_amd`'s, so the host reads the
face and the place within it that the guest's address names. The face is chosen at the half-way
points between whole numbers.

The pipeline reads a single cube's image descriptor - `TYPE` 11, `DEPTH` 5, the last layer on this
generation (`ac_descriptors.c:533-537`) - as six faces one after another. Each is a slice of the
surface, and addrlib places a slice's whole mip chain before the next (`gfx10addrlib.cpp`,
`ComputeSurfaceInfoMacroTiled`: `sliceSize` is the chain's, `surfSize` that times the slices), so
face `f` starts `f` chains in. The backend makes the six faces the layers of one cube-compatible
image and binds a cube view. A cube is bound only to a module that samples a cube, and a flat image
only to one that samples flat; a null descriptor in a cube slot is a cube of zeros.

Not read: an array of cubes (a `DEPTH` past 5), a cube through a half-border clamp, and a cube in
any format or tiling the flat image's block reader does not read.

**Why:** PPSA03416's pixel shader samples a `BC6_UFLOAT` cube in `4KB_S`, ten levels of 512 x 512,
and its draw was refused.

**Rejected:**
- Sampling the faces as a 2D array at layer `face`, `(s - 1, t - 1)`: the same texel at a face's
  middle, but it filters each face alone, where a cube filters across an edge into the next.
