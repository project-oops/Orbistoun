# D895 - A 2D array access to a one-layer image reaches layer zero

**Status:** decided
**Date:** 2026-10-19

A compute program may fetch from or store to an image through the `2D_ARRAY` dimensionality,
taking the layer from the address element after its two coordinates and any level from the one
after that. The translation addresses the image as the two-dimensional one it is, at the
coordinates given. Layer zero is the image. Any other layer is out of bounds, as it is on the
hardware for a one-layer view: a fetch there reads zero, and a store there writes nothing.

The host binds such an image only where its descriptor views one layer, the first: `DEPTH` and
`BASE_ARRAY` (`SQ_IMG_RSRC_WORD4`, bits 0-12 and 16-28, `gfx10-rsrc.json`) both zero. The type may be
`2D` or `2D_ARRAY`. A view of more layers, or of a later one, is refused by name.

**Why:** Grand Theft Auto V's compute copy loads two image descriptors and rewrites their type to
`2D_ARRAY` (`s_or_b32 s23, vcc_lo, 0xd0000000`) before it fetches from one and stores to the other,
its layer taken from its user data. The images it copies are two-dimensional, so the layer it names
is the first. A device test copies through a 2D-array access at layer zero, and at layer one the
destination keeps every texel.

**Rejected:**
- Binding every image a 2D-array access reaches as a host 2D array of its layers: the images this
  meets have one layer, and a layered surface's slices are placed by addrlib's slice pitch, which
  is read only when a program needs more than one.
