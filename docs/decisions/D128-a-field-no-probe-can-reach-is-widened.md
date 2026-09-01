# D128 - A field no probe can reach is widened from its family, and the claim is checked


**Status:** decided (2026-08-21) - the log line is a check now

`orbistoun-gen operands` now reconciles field widths within a family before writing, and
prints every adoption.

Usually a too-narrow field means a weak probe and the cure is a better one. Sometimes no
probe exists: `v_cndmask_b32` takes a sixty-four-bit mask as its third source, a mask is
always a scalar pair, and the highest value that field can legally hold is the execution
mask's code - so the top bits are unreachable by any instruction the assembler will emit.
Left alone it warned on every run, and **a check that always warns is a check nobody
reads**.

Only *widths* are reconciled. Two readings that disagree about kind or scale decode
differently, are a real ambiguity, and stay a warning - that distinction is what stops
this becoming a way to paper over a genuine one.

Verified by the case it was written for: the same run that adopted the width solved every
other opcode unchanged.

### The distinction is checked rather than asserted

The weakness was named when this was written and left open: *"sometimes no probe can"* and
*"usually the cure is a better probe"* look **identical** in the output, and the only thing
separating them was a line in a log. A line in a log is not a check, and this project has
already learned twice today that a too-narrow field is usually a weak probe.

It is answerable mechanically. Put a value in that operand which would need the wider
field, ask the assembler, and read the encoded field back. If a value that needs those bits
assembles, a probe could have been written and the adoption was avoidable. If none does,
the operand genuinely cannot reach there.

Run against this target it confirms the original claim rather than overturning it: both
adoptions - the conditional move's mask source and the carry-in form's - are constrained to
scalar pairs, and nothing an assembler will emit reaches those bits.

**A second defect, found on 2026-08-21 and the mirror of the first.** The check rebuilt its
trial instruction from the *parsed* operand list, which has modifiers stripped. For a
family whose modifiers are mandatory that produces something illegal:
`tbuffer_load_format_x v1, v200, s[8:11], s3` is refused outright, because a typed access
needs a `format:` and an addressing mode. So every candidate was refused and the check
reported "no instruction can reach those bits" about an address register field that any
`v200` reaches.

Same failure as the original, pointing the opposite way: that one said *avoidable* about an
unreachable field, this said *unreachable* about a probeable one. Both send someone to do
work that cannot succeed.

Fixed by keeping the printed text on each sample and substituting one token in place, so
the rebuilt instruction differs from a real one in exactly the operand under test. Worth
recording as a pattern rather than a bug: this check reasons about what the assembler
*will not* accept, and every silent way of making an instruction invalid reads as evidence.

**The first version of the check was wrong, and wrong in the dangerous direction.** It
stopped at "did it assemble" and reported every adoption as avoidable, because
`s[100:101]` assembles perfectly well - and its code is 100, which fits in seven bits and
needs nothing wider. That version would have sent someone to widen a probe for a field no
instruction can reach, which is precisely the advice this exists to avoid. The fix is that
it now reads the field back rather than trusting that the attempt succeeded.

It is checked in both directions, because a check that can only stay silent is worth
nothing: asked about an ordinary source, which accepts a vector register whose code is 256
and therefore needs the ninth bit, it says so.

