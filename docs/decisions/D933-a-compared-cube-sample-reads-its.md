# D933 - A compared cube sample reads its direction against the reference

**Status:** decided
**Date:** 2026-10-20

A depth-compare sample may name a cube (`image_sample_c*` with `dim:SQ_RSRC_IMG_CUBE`). It then
translates to a depth cube image sampled against its reference. The address elements are the
reference, then the face's two coordinates, then the face. The coordinates and the face become a
direction as a plain cube sample's do (D811). The depth target's six drawn slices bind as the cube
(D918), compared by the sampler's function (D817).

The face is the element after the face's coordinates, wherever the reference and any texel offsets
put them. Taking it from a fixed position read a coordinate as the face once a reference came first.

**Why:** Grand Theft Auto V's loading-screen lighting compares its point lights' shadow cubes with
`image_sample_c_lz ... dim:SQ_RSRC_IMG_CUBE`. Two pixel shaders were refused at that one
instruction, and the lighting segment, with every submission behind it, never ran.
