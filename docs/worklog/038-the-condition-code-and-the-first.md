# The condition code, and the first compiled shader to translate


`control.bin` translates. It is the only fixture that is compiler output containing real
control flow - a scalar compare, a branch on the condition code, a branch on the
condition mask, and a backward branch on the execution mask - and it is the first shader
not written here to survive the whole path. The census went from 0 of 10 complete to 1 of
10, and from 77 translatable instructions to 81.

Getting there needed the scalar condition code: one private word that the `SOPC` compares
write and the `scc` branches read. Both models hold one, because unlike a lane mask it is
not per-lane - one bit for the whole wavefront - so the per-lane model represents it
exactly and a shader using only the condition code stays on the fast model.

**Surprises.**

- **The agreement check caught a probe fault by itself, for the first time.**
  `s_cmp_ge_i32` and `s_cmp_le_i32` solved their second source at seven bits where their
  four siblings got eight, because no sample put an inline constant in that slot. This is
  the fifth occurrence of that exact fault and the first found by a tool rather than by
  reading generated rows side by side. The check was written two units ago for precisely
  this and had reported nothing since; it is now worth what it cost.

- **The branches were missing from `SUPPORTED`, and the test for that did not notice.**
  They are consumed by the block splitter rather than by the instruction dispatcher, so
  listing them felt wrong - and leaving them out made the worklist rank them as blockers
  of a shader that already translated. `control` was complete and reported as incomplete.

  The agreement test exists to stop exactly this drift, and it compares the two over a
  hand-written list of encodings that contained no branch. **A test that enumerates its
  own inputs finds drift only in the cases somebody thought to enumerate.** That is a
  sharper failure than the missing entry it was guarding.

- **A test had to be deleted because the thing it asserted became false.** `a_branch_on
  _the_scalar_condition_code_is_refused` was correct when written and correct to remove;
  it is now three tests asserting the branch works, including one whose only job is the
  signed/unsigned distinction. Worth noting because a refusal test passing is
  indistinguishable from a feature test passing, and only one of them should survive the
  feature arriving.

**Not done.** Nine fixtures still incomplete, blocked mostly on VOP3 arithmetic
(`v_fma_f32`, `v_mul_f32_e64`, `v_rcp_f32`, `v_div_scale_f32`) and on the families that
need a resource model - `exp`, MTBUF, VINTRP, MIMG. The first group is ordinary work with
a real oracle. The second cannot be validated without a submission from a title.

