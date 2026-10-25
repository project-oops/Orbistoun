# D978 - A lane write reaches its lane whether it runs or not

**Status:** decided
**Date:** 2026-10-25
**known_by:** published

`v_writelane_b32 v, s, lane` writes `s` into lane `lane` of `v` whether that lane runs or not. The
instruction ignores the execution mask (AMD's published RDNA 2 instruction set, `V_WRITELANE_B32`).
Every model writes it that way: one invocation holding the wave, one lane an invocation (D880),
two lanes an invocation (D973), and the host evaluator. The translation reads each lane's old value
and selects the written word into the named lane. It stores that result into every modelled lane,
past the execution mask (`Model::write_vector_lane_unmasked`). A lane the model knows never runs is
written too, because `v_readlane_b32` still reads it.

Until now the write went through the mask, as other lane writes do. That relied on an assumption:
a program writes only the lanes it later reads, and those lanes run.

**Why:** Grand Theft Auto V's full-screen ray traversal at `0x74005b7e8600` keeps its node stack
in lanes of `v29`-`v31`. The stack's depth, `s86`, is the lane index. Each push writes the depth's
lane while the execution mask holds only the lanes the last box test sent on. A push whose lane
was outside that mask was dropped. The pop then read an older entry back: a node from another
level, or a pointer the stack never held. That entry sent the walk into a leaf list of the wrong
bottom-level buffer. The leaf loop ends only on an entry whose last word is negative. Reading past
the records gives zeros, so the loop never met that entry. Under D949's cap of 4096 (run 344),
every dispatch was cut at the leaf loop's backward branch, `+0x1c04`, and one at the outer loop's,
`+0x1c10`. Without the cap, runs 341-343 lost the device.

The device test `a_lane_write_reaches_a_lane_that_does_not_run` reproduces this. A wave of 64 runs
only lane 0 while it writes lanes 7 and 40, then reads lane 40 back. Before the fix, both models
left the lanes unwritten. After it, both write them.

**Rejected:**
- Keeping the masked write and widening the mask around stack pushes: that is a rule for one
  program's shape, and the instruction's own definition already covers every program.
