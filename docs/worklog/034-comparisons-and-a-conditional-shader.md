# Comparisons, and a conditional shader with no branch in it


Float comparisons translate. `v_cmp_lt_f32`, `v_cmp_eq_f32` and `v_cmp_gt_f32` compare
every lane and assemble the answers into the condition mask, which a shader then ands
into the execution mask - and that is an if-branch, complete, with no branch instruction
involved. Predicated execution needs the mask, not the jump. Thirty-six execution tests,
all on a real device.

Two of those tests exist because a plausible translation passes without them.

- **Comparisons are on floats, so the registers are bitcast first.** Comparing the
  integers agrees with the float comparison on every non-negative pair and orders
  negatives backwards, so `-1.0 < 1.0` is the only shape that separates them. A test
  using positive values would pass either way.

- **A comparison writes `vcc`, not `exec`.** Every other test ands the result into `exec`
  immediately afterwards, so a translation that wrote straight to `exec` would pass all
  of them. The test that catches it compares falsely and then does *not* and it in: every
  lane must still be active.

The lane model refuses comparisons for the same reason it refuses mask writes - a
comparison produces one bit per lane and that model has nowhere to put sixty-four of
them. Answering with the one lane it has would be a mask claiming the other sixty-three
agree. `Fidelity::Auto` routes such a shader to the wavefront model, which is now tested
rather than asserted.

**Surprises.**

- **The comparison's destination is not in the encoding.** `vcc` is printed and occupies
  no bits, so the solver reported the opcode unsolvable - one operand with no field fails
  the whole thing. Recording it as implicit rather than dropping it keeps a decoded
  comparison honest about what it writes; dropping it would have been easier and would
  have made every report of a comparison omit its entire effect.

  The evidence that it is implicit rather than merely unvaried: the assembler refuses any
  other destination for this form, and the 64-bit form of the same comparison does encode
  one and does solve. Without that check the same code path would happily record "the
  probes were lazy" as "the field does not exist".

- **One comparison solved and its sibling did not, from identical probes.** `v_cmp_eq`
  was fine; `v_cmp_lt` was ambiguous. The cause was the opcode bits: a nine-bit window
  overlapping the opcode field read the vector register plus 256, which is that same
  register in the shared numbering - and the ninth bit is 1 for `lt` and 0 for `eq`. So
  `lt` had two readings that agreed on every sample and always would.

  The fix is general and obvious in hindsight: **a candidate operand field may not overlap
  the opcode.** Those bits are already spoken for. What made it findable was having a
  sibling that worked - two opcodes identical in shape, one solvable and one not, points
  at the opcode rather than at the probes.

- **The same register arrives under two names.** `vcc_lo` when a source field's code goes
  through the operand table, `vcc` when the implicit destination carries the reference's
  text. Both correct, neither carrying the width. Normalised at the point of use rather
  than by rewriting either table to agree with the other, because each is recording what
  it observed.

- **A bulk-edit script mangled two string continuations into runs of spaces.** Same root
  cause as the earlier capture-group failure: a backslash that means "continue this line"
  in Rust and "escape the next character" in the generator. Caught by a test asserting on
  message content, which is the only reason it was not shipped as a message with
  twenty-two spaces in the middle of it.

**Not done.** Branching. The mask can be computed, honoured, and now *derived from a
comparison*, but a jump taken when no lane survives cannot be expressed - SPIR-V demands
structured control flow and the guest's is implied. That remains the substance of D098
and it is a design problem. Comparisons are also uniform across lanes for now: every lane
compares the same registers because no instruction yields a lane index, so the mask is
all-ones or all-zero. Per-lane divergence needs that source first.

