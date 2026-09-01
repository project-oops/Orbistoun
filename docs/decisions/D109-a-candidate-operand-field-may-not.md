# D109 - A candidate operand field may not overlap the opcode


**Status:** assumed

The solver discards any candidate field that overlaps the family's opcode bits. Those
bits are already spoken for: the encoding table extracts the opcode from them, so no
operand can also live there.

Not tidiness. `v_cmp_lt_f32_e32` was unsolvable without it. Its second source is an
8-bit vector register at bit 9, and a *9*-bit window at the same position reads that
register plus 256 - which is exactly the same register in the shared source numbering.
Both readings explained every sample and always would, because the ninth bit belongs to
the opcode and is 1 for this opcode in every instruction that has it.

What made the cause findable was a sibling: `v_cmp_eq_f32_e32` has a 0 in that bit and
solved without complaint. Two opcodes of one family, identical in shape, one solvable and
one not - which points at the opcode bits rather than at the probes.

Verified not to have moved any previously-solved layout.

**This rule was half the rule.** The opcode is not the only thing in the first word that
an operand cannot overlap, and the missing half cost the two most common instructions in
the set. Superseded by [D202](#d202--a-candidate-operand-field-may-not-overlap-anything-the-encoding-table-already-reads),
which generalises it; the reasoning here is still the reasoning there.

