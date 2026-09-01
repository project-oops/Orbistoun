# D205 - Operands spelled as names have their codes measured, not written down


**Status:** decided (2026-08-21)

Export and interpolation were the last two families with no probes, and the reason they
had none is that their operands are not registers or numbers. An export names a target -
`mrt0`, `pos1`, `param4` - an interpolation names an attribute and a channel as one token
`attr3.y`, and one of them names a parameter, `p10`. The solver cannot fit a field to a
value it does not have.

The obvious fix is to write down that `mrt0` is 0 and `pos0` is 12. This does not, for the
reason nothing else here is transcribed: the number cannot be checked without the document
it came from, and being wrong means reporting the wrong export target indefinitely.

**They are measured instead.** `derive_symbolic_codes` assembles the same instruction once
per spelling, holding everything else constant. Whatever stays the same is not the field;
the bits that move are, and each name's code is what it holds there - the same move
`orbistoun-gen encodings` uses to find a family's mask. Fifty codes, one round trip.

Enumerating the candidate *spellings* is not the same as transcribing their values: a
spelling that does not exist is refused and drops out, and one that does has its code read
off the encoding.

**Kept apart from the shared source numbering.** `mrt0` is export target zero, not scalar
register zero. Offering it as a source code would let the decoder report an export as
writing to `s0`.

### Three things the solver found that were not the target

**The channel selector is two bits and the width search started at five.** Register fields
are five to nine bits and immediates were 16, 20, 21 or 32 - a selector is neither. All
three interpolation opcodes reported as unsolvable, which reads as a gap in the probes and
was a gap in the search.

**The attribute number is bounded at 31, and the field really is five bits.** Probing higher
was tried on the assumption it was six; the assembler refused `attr47`, `attr52` and
`attr61` as out of bounds. A better answer than the one being looked for, and the reason to
ask rather than assume.

**Width reconciliation was keyed by kind, so it could not see its own disagreement.**
`v_interp_p1` reads bits 7:0 as a vector register and `v_interp_mov` reads the same bits as
a parameter selector with three legal values, so it solved two bits wide. Keyed by kind
those never met; the consistency check, which ignores kind, then reported a disagreement
nothing could act on. A field's width is a property of the encoding and how an opcode reads
it is not, so the key drops the kind.

### The differential test found a real omission the moment it had something to compare

`normalise` took the first word of each comma-separated piece, to strip trailing modifiers.
The reference does not put a comma between every operand - an export prints `mrt0 v0` -
so the rest of the piece was silently dropped. Invisible while those families decoded
nothing, and a failure the moment they decoded something.

That is the D202 lesson arriving from the other side: the vacuous pass hid a defect in the
comparison itself, not only in the thing compared.

