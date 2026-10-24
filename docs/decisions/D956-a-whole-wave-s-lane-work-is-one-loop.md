# D956 - A whole wave's lane work is one loop over its lanes

**Status:** decided
**Date:** 2026-10-24
**known_by:** measured

A compute dispatch translated as a whole wave in one invocation emits each eligible instruction's
per-lane work once, inside a loop over the wave's lanes, rather than once for every lane in
straight-line code. The lanes run in the same order as before, one after another. An instruction is
eligible when it reaches its lane only through the model's per-lane reads and writes (`LANE_LOOPED`
in the translator's model): buffer and LDS accesses and atomics, the common vector ALU forms and
their SDWA forms, `v_movrels` and `v_movreld`. A compare carries its result mask around the loop. A
DPP form and `ds_bpermute` run as separate loops for gathering, computing and writing back, so every
lane reads before any lane writes. Instructions that name a lane themselves (`v_readlane`,
`v_writelane`, the other permutes, `v_mbcnt`) and 64-bit compares stay lane by lane.

**Why:** Grand Theft Auto V's first in-world frames sat for up to twelve minutes at a time inside the
host driver's compiler. The program at `image+0x3d22d00`, 681 instructions, translated to 4,319,266
words of SPIR-V, 744 s to compile; five more of its compute programs were 0.75 to 3.9 million words.
The module grew with the program's length times 64: a wave32 build of the same program was exactly
half the size, and one four-dword buffer store was 58,000 to 70,000 words. After the dispatch at
`image+0x3dc3400` compiled, its first run lost the device. With the loop the six programs are 40,000
to 249,000 words, at most about 205 words a guest instruction.

**Rejected:**
- Unrolling only some lanes, or a smaller wave for the model. The size would still follow the
  width, and a program written for 64 lanes reads them all.
- One invocation per lane for these programs. They read other lanes and share the LDS between
  lanes in one step, which is why they are modelled as a whole wave.
