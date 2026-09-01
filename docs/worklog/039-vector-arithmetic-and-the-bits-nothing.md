# Vector arithmetic, and the bits nothing was looking at


The vector ALU's arithmetic translates: add, subtract, reverse-subtract and multiply in
both encodings, multiply-add, fused multiply-add, reciprocal, and the conditional move.
The census went from 81 to 93 of 126 instructions and from 1 to 2 of 10 fixtures
complete. Fifty execution tests, all on a real device.

**The substance was the modifiers, not the arithmetic.** The long form carries per-source
negate and absolute flags in bits that neither the operand layout nor the encoding table
describes - both layers ignore them correctly, and both are the layer that would have
been asked. A translator that read the operands and stopped would emit `a + b` where the
guest wrote `a + -b`, in a shader that runs, for every subtraction a compiler expressed
that way. Three tests: negate alone on a positive source, absolute alone on a negative
one, and both together where only the *order* separates the answers.

Clamp and the output multiplier are refused rather than ignored, and `Instruction` gained
`second_word` so a translator can reach fields the decoder does not model.

**Surprises.**

- **`v_cndmask_b32` could not be solved, and then could not be solved a second time for a
  different reason.** Its first two sources are always vector registers in the probes, so
  an eight-bit direct index and a nine-bit shared-numbering reading both explained every
  sample - and those decode differently, so the solver refused. The fix was a source that
  is not a vector register, which the constant-bus rule nearly forbids: the long form may
  read one value off that bus and the mask has taken it, so a *scalar* first source is
  rejected by the assembler. Inline constants are exempt, and that exemption is the only
  reason this was separable at all.

- **Then its third source could not be widened by any probe.** The mask is always a
  scalar pair, so the field's top bits are unreachable by any legal instruction. The
  family agreement check - which had just earned its keep by catching two real probe
  faults - would have warned about it forever, and a check that always warns is a check
  nobody reads. Widths are now reconciled from the family and every adoption is logged;
  kind and scale disagreements still warn, because those decode differently and are a
  real ambiguity.

- **A test asserted nothing for an hour and passed the whole time.** The pipeline's
  "a shader that does not translate is reported" test used a vector subtract as its
  unsupported instruction - and vector subtract was implemented in this unit. It now uses
  an export, which is blocked on a render target rather than merely unimplemented, so it
  cannot become supported by accident. **A test that depends on something being missing
  needs to depend on something that will stay missing for a reason.**

- **An extraction cut the wrong range twice and duplicated two match arms.** Both times
  the compiler caught it immediately - unreachable pattern, then dead code - which is the
  argument for doing this kind of surgery in a language that checks. It cost three
  attempts and no correctness.

**Not done.** The division helpers (`v_div_scale_f32`, `v_div_fixup_f32`,
`v_div_fmas_f32`) are the Newton-Raphson sequence, with special-case behaviour around
denormals and overflow that is worth being careful about; they are the last VOP3 blockers
and deliberately left. `v_rcp_f32` is translated as an exact division where the guest's is
an approximation - more accurate than the hardware, which cannot turn a correct frame
into a wrong one but will show up in a bit-exact framebuffer comparison. `v_mad_f32` and
`v_fma_f32` translate identically, so the fused one is the less faithful of the two.

