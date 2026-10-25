# D956 - A whole wave's lane work is one loop over its lanes

**Status:** decided
**Date:** 2026-10-25
**known_by:** measured

A compute dispatch translated as a whole wave in one invocation emits its lane work inside loops over
the wave's lanes, rather than once for every lane in straight-line code. Consecutive eligible
instructions in a block share one loop. An instruction is eligible when it reaches its lane only
through the model's per-lane reads and writes (`LANE_LOOPED` in the translator's model): buffer,
global, image and LDS accesses and atomics, the ray intersections `image_bvh_intersect_ray` and
`image_bvh64_intersect_ray` (a read of memory), the common vector ALU forms and their SDWA forms,
`v_movrels` and `v_movreld`. Instructions that emit nothing, such as `s_waitcnt` and `s_nop`, sit
inside a run. A compare carries its result mask around a loop of its own. A DPP form and
`ds_bpermute` run as separate loops for gathering, computing and writing back. Instructions that
name a lane themselves (`v_readlane`, `v_writelane`, the other permutes, `v_mbcnt`), 64-bit compares
and scalar instructions stay outside the loops.

The hardware runs every lane's instruction before any lane's next, and a loop runs one lane's whole
run before the next lane's. The two agree while no lane sees another's work within the run.
Registers are each lane's own, but memory and the LDS are shared. So a run ends before a store or
atomic where it already holds an access to them, and before a load, sample or gather where it
already holds a store or atomic. A run also ends at anything that is not eligible, and at the end of
its block.

**Why:** Grand Theft Auto V's first in-world frames sat for up to twelve minutes at a time inside the
host driver's compiler. The program at `image+0x3d22d00`, 681 instructions, translated to 4,319,266
words of SPIR-V and took 744 s to compile. The module grew with the program's length times 64. With a
loop for each instruction the six programs were 40,000 to 249,000 words. Its full-screen pass at
`0x74005b86f800` still had 1,637 lane loops at 388,271 words and took 529 s to compile. The driver's
time follows the number of loops much faster than the number of words. Runs of lane work bring the
pass to 501 loops and 46.5 s.

A ray intersection emitted once a lane is the same growth. The program at `0x74005b7e8600` has six,
each in a block of 17 instructions that became about 1,305,000 words of straight-line code, 83% of a
9,410,362-word module the driver never finished compiling. In one loop over the lanes a wave's
intersect is 5,721 words where it was 352,205, and each lane still meets its own child
(`every_lane_tests_its_own_ray`).

**Rejected:**
- Unrolling only some lanes, or a smaller wave for the model. The size would still follow the
  width, and a program written for 64 lanes reads them all.
- One invocation per lane for these programs. They read other lanes and share the LDS between
  lanes in one step, which is why they are modelled as a whole wave.
- Fusing across memory accesses freely. One lane's load would see another's store that the
  hardware had not yet made, or miss one it had.
