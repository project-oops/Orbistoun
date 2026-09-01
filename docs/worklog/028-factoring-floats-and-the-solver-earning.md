# 2026-08-20 - Factoring, floats, and the solver earning its refusals


**Done.** Three units, gate green after each.

- **Factored the backend seam** (`model.rs`). Instruction dispatch is shared; the two
  models differ in four methods. Deferred until two implementations existed, exactly as
  planned, and the seam turned out narrower than expected.
- **Floating-point arithmetic**: `v_add_f32` and `v_mul_f32`, written once and available
  at both fidelity levels. Worklist moved **45 to 51 of 106**.
- **Memory operand layouts solved**: all fifteen probed opcodes, up from ten, including
  every scalar load and both flat accesses.

**Verified.** 13 execution tests on hardware, including the two models agreeing on
float arithmetic. The float work passed first try.

**Surprises.**

- **The solver refused the wide loads for a real reason, and the fix was not to weaken
  it.** A multi-register operand names an *aligned* group, so a scaled reading of some
  other field always fits alongside the unscaled reading of the real one. Preferring the
  unscaled reading is safe precisely because where scaling is genuine the unscaled
  reading does not fit at all - a base field holding 3 for register 6 is explained by
  scale two and nothing else.

- **`off` is not an operand.** The disassembler prints it among them, so the solver
  looked for bits encoding the word "off" and gave up on every flat access. Cache hints
  are the same. They are modifiers and are now filtered before solving.

- **The store's remaining ambiguity was the sharpest one yet, and entirely real.** `off`
  encodes "no scalar base" as all ones, so the bit above the data field was set in
  *every* sample - and a nine-bit window therefore read exactly data-plus-256, which is
  indistinguishable from the shared operand numbering. No number of extra `off` samples
  could have separated them. Probing with a real scalar base varies that bit and solved
  it immediately.

  Worth keeping as the general lesson: when samples cannot distinguish two readings, the
  answer is a *different kind* of sample, not more of the same kind. The solver was
  right to refuse and refusing is what made the diagnosis possible.

- **Comparing floats tripped a lint that was right in general and wrong here.** The
  assertions are about bit patterns, so they compare bits - which also removes the
  question of a tolerance that does not apply.

**Not done.** Memory operands are *decoded*, not *translated* - no guest memory buffer
exists yet, so every load and store is still refused. That is the next unit and the top
five worklist entries are all waiting on it.

