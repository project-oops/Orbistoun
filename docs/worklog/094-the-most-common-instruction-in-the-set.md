# The most common instruction in the set had no operands, and every test was green


Three opcodes were failing to solve. Chasing them was meant to be housekeeping.

`v_mov_b32_e32` and `v_rcp_f32_e32` had **no operand row at all** - a move decoded to a
mnemonic and an empty list. The cause is D109's rule stated too narrowly. VOP1 keeps its
destination at bit 17 as an eight-bit vector register, and a nine-bit window at the same
place reaches bit 25, which is the low bit of the family mask and permanently 1 - so it
reads `v0` as 256, which is precisely `v0`'s code in the shared source numbering. Two
readings, every sample explained by both, and the bit that separates them is a constant.

The solver refused, which was correct. It was correct in a way nobody could see.

Fixed by generalising the exclusion from the opcode field to `mask | opcode` - everything
the encoding table already reads (D202). 68 of 71 solved before, 70 after, and re-solving
removed **zero** existing rows.

### Surprises

**D109's own discovery method could not have found this one.** D109 was found because
`v_cmp_lt_f32_e32` would not solve while its sibling `v_cmp_eq_f32_e32` did - one opcode
bit apart, which pointed straight at the opcode field. There is no such asymmetry here:
every VOP1 instruction shares the same family bits, so every one of them is ambiguous in
the same way and there is no sibling to compare against. The technique that found the rule
was blind to the half of it that was missing.

**The differential suite was green throughout, and structurally could not have been
otherwise.** `every_decoded_operand_appears_in_the_reference` iterates the operands the
decoder produced and looks each up in the reference. Produce none and the loop body never
runs. A one-directional check cannot see an empty answer - and "no operands" is the exact
output an unsolved opcode gives.

That is the part worth keeping. The overlap rule fixes two opcodes; the missing converse
is a whole class. There is now a test asserting the set of instructions decoding nothing is
*exactly* a written-down list, twelve entries at the time and seven now, each with its
reason. Closing a gap fails
until the entry is deleted; opening one fails until it is added and justified.

**`s_waitcnt` is the one still unsolved, and it should be.** Its operand is three
independent counters packed into one sixteen-bit field, and the reference prints only the
ones not at their maximum - so operand text and encoded field have no positional
correspondence for the solver to find. Refusing is the right answer; it needs its own
handling in translation, not a field.

