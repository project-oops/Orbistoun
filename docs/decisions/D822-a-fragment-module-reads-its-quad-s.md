# D822 - A fragment module reads its quad's neighbours through the host's quad

**Status:** decided
**Date:** 2026-10-13

Amends D716. A fragment module still simulates one lane, one pixel per invocation. An
instruction that reads another lane of its quad - a DPP16 quad permute of a one- or two-source
vector instruction's first source - reads it from the host invocation at that place in the host's
own 2x2 quad, by `OpGroupNonUniformQuadBroadcast`, and finds its own place from the low two bits
of its subgroup index. The source lane's execution bit is read the same way, so the inactive-lane
rules (`FETCH_INACTIVE`, `BOUND_CTRL`) hold. Only a module that has such an instruction declares the
quad capability and the subgroup index, found by a scan before the entry point is written.

The permuted source is staged in a 257th vector register, one past every register an instruction
can name, and the instruction translated as though it named that register. Every model shares that
path; one that holds every lane reads the neighbour by index.

**Why:** a derivative is a quad permute - compilers emit `v_mov_b32` with `quad_perm:[0,0,2,2]`
then a subtract with `[1,1,3,3]` - and D716's premise, that no translated instruction reads
another lane, ends there. The guest's quad and the host's are the same object: Vulkan numbers a
fragment quad top-left, top-right, bottom-left, bottom-right, as the guest does, and runs helper
invocations so every place in it exists. `gl2-probe` (GLTP00001) checks its own pixels, and its
`derivatives` case passes once this is in.

**Rejected:**
- Simulating four lanes per fragment invocation: each would need its own interpolated inputs,
  which the host computes only for the pixel it is.
- Declaring the quad capability on every fragment module: it would make every fragment shader
  need subgroup support in the fragment stage for the few that permute.
- Rewriting the source operand in place: the instruction set has no spare register to name, and
  the permuted value must not overwrite one the guest reads later.

**Assumed:** that the host device supports quad operations in the fragment stage. Desktop drivers
do; nothing here checks it, and a device without it would fail to create the pipeline.
