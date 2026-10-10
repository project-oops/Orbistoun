# D785 - v_med3_f32 is the median of three, NaN as its min-max steps

**Status:** decided
**Date:** 2026-10-10
**known_by:** published (the median: ACO forms `v_med3_f32` from `max(min(x, upper), lower)` when
`lower <= upper`, `src/amd/compiler/aco_optimizer.cpp` `create_med3_cb`); assumed (a NaN source)

`v_med3_f32` writes the median of its three sources, each with its absolute and negate flags, as
`max(min(a, b), min(max(a, b), c))` built from the same `FMin` and `FMax` steps `v_min3_f32` and
`v_max3_f32` translate to. For three ordinary numbers that is the middle one whatever their order.
A NaN source takes whatever those steps make of it.

**Why:** PPSA03416 reaches a vertex shader using it as soon as it leaves its first scene. ACO emits
the instruction for exactly a clamp it can prove is a median, so the median is Mesa's meaning of
it; what the hardware does with a NaN source is not something Mesa says, and the steps are the
nearest translation that already exists here.
