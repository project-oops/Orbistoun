# D816 - a depth-compare sample reads a depth texture against its reference

**Status:** decided
**Date:** 2026-10-12

`image_sample_c_lz` is translated: a sample of a depth texture at level zero, compared against a
reference. Its address is the reference and then the coordinates, as ACO places a compare's
reference ahead of them (`aco_select_nir.cpp`, the MIMG address after any offset and bias). The
module declares the binding a depth image and samples it with `OpImageSampleDrefExplicitLod` at
level zero; the result, one float, is written to the one channel the mask names. A compare of a
cube, or of more than one channel, is refused.

The comparison and its filtering are the host sampler's, set from the guest's sampler descriptor:
`SQ_IMG_SAMP_WORD0.DEPTH_COMPARE_FUNC` (bits 14:12, `gfx10-rsrc.json:466`), never to always in the
order `SQ_TEX_DEPTH_COMPARE` numbers them (`gfx6.json:627-637`), which is the order a host's compare
operations take.

Until the host binds a depth texture with that sampler, a draw whose module samples one is refused
as an unbound texture, which names the texture's descriptor.

**Why:** PPSA03416's shadowed scenes, past its title screen, sample their shadow map this way (MIMG
opcode 47), and the fragment shader could not be translated.
