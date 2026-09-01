# The second sub-encoding, and a check that was lying


The carry-producing vector arithmetic translates - `v_add_co_u32`, `v_sub_co_u32`,
`v_addc_co_u32` - which is what sixty-four-bit address arithmetic is built out of. Sixty
execution tests on a real device. The census is unchanged at 102 of 126 and 3 of 10,
because the remaining blockers in that fixture are the division sequence.

**The reason this unit happened was an encoding trap, not breadth.** The long form has
two sub-encodings, and they put different things in bits 8 to 14 of the first word: one
has the per-source absolute-value flags, the other a second, scalar destination. The
modifier reader written two units ago read those bits unconditionally. `vcc` as a carry
destination is 106 - 1101010 - so its low three bits claim "the second source is an
absolute value", and an integer addition would silently lose the sign of an operand.

Nothing said which sub-encoding an opcode uses. The encoding table declines to give this
family a layout for exactly this reason and says so in a comment; the operand solver
reports the fields without naming the sub-encoding. The answer had to be written down
somewhere, and the translator is the layer that acts on it. There is a test whose only
job is that the misread would have changed the answer.

**Surprises.**

- **The identifier check produced a false failure, and it was mine.** Adding
  `OpLogicalOr` without its row in the shape table meant the check skipped that
  instruction - so the identifier it defined was never recorded, and the *next*
  instruction was reported as referring to nothing. The error named the wrong
  instruction and pointed at code that was correct.

  The original reasoning for skipping unknown opcodes was sound as far as it went:
  guessing an unknown shape would mean reading a literal as an identifier and rejecting a
  module that is fine. What it missed is that skipping affects the parts of the check
  that *depend* on the skipped instruction. An unknown opcode is now an error in its own
  right, checked first. **A check that degrades on unknown input has to be asked what it
  does with everything downstream of the unknown, not only with the unknown itself.**

- **rustfmt had reflowed the row I was trying to insert next to**, so the edit silently
  did nothing and the bug persisted through a fix that looked applied. Generated-looking
  code is not a stable anchor.

- **Six opcodes needed an inline constant in every source position, one at a time.** The
  same ambiguity `v_cndmask_b32` hit - an eight-bit direct index and a nine-bit
  shared-numbering reading both fit when every sample is a vector register. Separating one
  slot says nothing about the next, and `v_addc_co_u32` has five operands, so it took two
  rounds to notice its *first* source was still unseparated.

**The division sequence is refused rather than translated**, with a reason per instruction
(D124). It needs exponent thresholds and a special-case substitution table from the
published instruction set, and a guess would be exact for ordinary values and wrong at the
extremes - which survives every test anybody thinks to write and surfaces years later as
an artefact nobody can reproduce. Worth distinguishing from `exp`, which is blocked on a
subsystem that does not exist: this is blocked on documentation nobody has read yet, and
is much cheaper to unblock.

**Still red outside this side.** The workspace gate fails on
`crates/orbistoun-elf/examples/dyntags.rs`, untracked and belonging to the loader work.
All five crates here lint clean and pass with no device skips.

