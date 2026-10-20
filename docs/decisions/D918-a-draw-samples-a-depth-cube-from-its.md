# D918 - A draw samples a depth cube from its six drawn slices

**Status:** decided
**Date:** 2026-10-20

A draw may sample, as a cube, a depth-format image in a depth swizzle whose descriptor is a cube:
`TYPE` CUBE, `DEPTH` 5 and `BASE_ARRAY` 0. Each of that image's six faces is the depth target the
host drew at that slice of the surface (D909). The backend reads the six back and binds them as a
cube's faces in order, compared or read as the draw asks (D817).

A cube is refused where any face was never drawn here, where the faces were drawn at different
extents, or where its view is other than one cube's six faces. A flat depth texture is bound as
before.

**Why:** Grand Theft Auto V's loading-screen lighting samples point-light shadows from 16_UNORM
cubes in `64KB_Z_X`, which draws refused.
