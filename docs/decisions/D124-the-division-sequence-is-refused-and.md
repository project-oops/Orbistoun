# D124 - The division sequence is refused, and says which numbers are missing


**Status:** assumed

`v_div_scale_f32`, `v_div_fmas_f32` and `v_div_fixup_f32` are in `model::BLOCKED` with
individual reasons rather than translated.

They implement float division together: pre-scale an operand by a power of two so the
reciprocal that follows cannot overflow, refine, then scale back and substitute results
for the special cases - zero over zero, infinity over infinity, signed zeros, NaNs. The
thresholds that decide when to scale, and the table of substitutions, are stated in the
published instruction set and are not available here.

A first guess would be exact for ordinary values and wrong at the extremes, which is the
worst shape a numerical bug takes: it survives every test somebody thinks to write and
appears years later as a rendering artefact nobody can reproduce. Refusing costs one
fixture; guessing costs the ability to trust any division.

Worth distinguishing from the other entries in that list: `exp` is blocked on a
*subsystem* that does not exist, and these are blocked on *documentation* that has not
been consulted. The second is much cheaper to unblock, and the reason strings say so.

