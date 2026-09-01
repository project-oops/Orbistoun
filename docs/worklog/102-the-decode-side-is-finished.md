# The decode side is finished


Export and interpolation solved. **79 of 80 probed opcodes**, and the silent-empty
inventory is down from twelve entries to three.

Their operands are names rather than registers, so the codes were measured: assemble once
per spelling holding everything else constant, and read the bits that moved. Fifty codes in
one round trip, and enumerating spellings is not transcribing values - a spelling that does
not exist is refused and drops out.

### Surprises

**The attribute field is five bits and I went looking to prove it was six.** Probed
`attr47`, `attr52`, `attr61`; the assembler refused all three as out of bounds. The
assumption was wrong and the answer was better than the one being looked for.

**A two-bit field could not be found because the search started at five.** Register fields
are five to nine bits, immediates were 16 and up, and a channel selector is neither. Three
opcodes reported unsolvable for want of a width in the list - which reads as a gap in the
probes and was a gap in the solver.

**Width reconciliation was keyed by kind and therefore blind to its own disagreement.**
`v_interp_p1` reads bits 7:0 as a register, `v_interp_mov` reads them as a selector with
three legal values. Keyed by kind they never met; the consistency check ignores kind and
reported a disagreement nothing could act on. Two parts of one tool disagreeing about what
a field is.

**The differential comparison had a defect the whole time and nothing could see it.**
`normalise` took the first word of each comma-piece to strip trailing modifiers, so
`exp mrt0 v0` lost `v0`. It could not fail while exports decoded no operands - there was
nothing to compare. Solving the layout turned a silent omission into a red test on the
first run. The D202 lesson from the other side: a vacuous pass can hide a fault in the
*check*, not only in the thing checked.

**And one loosening, recorded rather than hidden.** `null` is an export target in an export
and a special register in a vector instruction, and the reference prints it in both.
Replacing the token with its code broke every instruction using the register sense, so the
code is offered alongside the spelling - which accepts a decoded `9` where the reference
said `null`. Real, small, and cheaper than threading the family through the comparison.


