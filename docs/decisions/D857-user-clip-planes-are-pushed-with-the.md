# D857 - User clip planes are pushed with the draw, not compiled in

**Status:** assumed
**Date:** 2026-10-14

**Amends:** D830 - the planes were specialization constants of the mesh module.

A translated mesh module reads the user clip planes (`PA_CL_UCP_0` to `_5`) from the push-constant
block at entry: word `4 * plane + axis` from offset zero. A mesh module's user data arrives in the
draw-data buffer (D718), so the block's geometry half is free, and a mesh pipeline's block carries
the planes there when it is bound. The fragment half is the fragment stage's user data, as before.
The planes are part of the key a pipeline is kept and batched by, and not of its shape (D856).

**Why:** a specialization constant is compiled into the device pipeline, so each new set of planes
was a new pipeline. Neverball clips its reflections against planes that move with the scene: 38
sets in 20 seconds, and with D856 alone 852 of its 1,226 builds still created a device pipeline. With
the planes pushed, 45 did, and it went from 4.4 to 6.5 frames a second.

**Rejected:**
- A uniform buffer of planes: a further binding for six vectors the push block already has room
  for in every mesh pipeline.
