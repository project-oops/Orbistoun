# D555 - Both interpolation halves answer the whole value

**Status:** assumed
**Date:** 2026-09-04

`v_interp_p1_f32` and `v_interp_p2_f32` each translate to a read of the fragment `Input`
variable, the fully interpolated attribute, and their barycentric operands are ignored.
`v_interp_mov_f32` is refused.

**Why:** the guest pair computes one interpolated value in two steps, and SPIR-V has no such pair:
the host pipeline interpolates and the input variable is the result. Treating `p2` as a no-op
breaks as soon as the two halves do not share a destination. A shader that uses `p1`'s
intermediate for anything but feeding `p2` gets a different number, which is the edge this
assumes away. `v_interp_mov_f32`'s selector has no citable encoding.

**Rejected:**
- `p2` as a no-op on `p1`'s result: wrong when the halves use different destinations.
- Reproducing the two-step arithmetic from the guest's I and J: the host does not expose its own barycentrics that way.
- Translating `v_interp_mov_f32` as the attribute value: right for one selector of three and silently wrong for the others.
