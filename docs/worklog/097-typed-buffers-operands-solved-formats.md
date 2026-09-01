# Typed buffers: operands solved, formats measured


MTBUF now has operand layouts for all five opcodes, identical to the untyped family's, and
a format table derived from the assembler.

Getting the operands took three rounds, and every round failed the same way for a different
reason: two readings fitted every sample and nothing could separate them.

**Round one.** Four of five opcodes unsolvable. The resource operand has a false reading at
bit 14 with no scaling, because `base = (word >> 16 & 0x1F) * 4` is arithmetically
identical to `(word >> 14) & 0x7C` whenever bits 15:14 are zero - and those bits are the
top of the *data* register field. Every data register in those probes was below 64. The one
opcode that solved was the one that happened to contain a `v200`.

**Round two.** Two still unsolvable, one bit along. The resource group index is `base / 4`
at 20:16, so its low bit is bit 16, which is also the top of a nine-bit window over the
data register at 8. Every resource base in those two groups was congruent to 4 mod 8, so
that bit was always 1 and the window read `v4` as 260 - exactly `v4`'s code in the shared
source numbering. The same shape as D202's VOP1 fault, arriving through a different field.

**Round three.** All five solved, and the generator started reporting the fields it had
widened from the family rather than measured. Taking its advice - address registers past
128, scalar offsets past 64, and a literal `0` for the offset, which is inline constant 128
- removed every adoption.

### Surprises

**The widening check was giving confidently wrong advice, and had been since MTBUF
existed.** It rebuilds an instruction from the parsed operand list, which has the modifiers
stripped - and `tbuffer_load_format_x v1, v200, s[8:11], s3` is *refused*, because a typed
access needs its `format:[...]` and an addressing mode. So every candidate was refused and
it reported "no instruction can reach those bits" for a field any address register reaches.
That is D128's failure exactly, mirrored: D128's first version said "avoidable" for an
unreachable field, this said "unreachable" for a probeable one. Samples now carry the
printed text verbatim and substitution edits one token in place.

**MTBUF's scalar offset solved seven bits wide where the untyped family solves eight, and
nothing flagged it.** Width reconciliation compares within a family, and every typed opcode
agreed with every other typed opcode. The disagreement was across families, which nothing
looks at. Real shaders use `0` there constantly and that is inline constant 128, so the
eighth bit is reachable and ordinary - the probes just never used it.

**The format sweep cannot see its own default.** A code that prints no name is either the
default or reserved, and the numeric sweep cannot tell those apart. Asking the opposite
question - assemble each candidate *name*, see which code comes back - recovered code 1 as
`BUF_FMT_8_UNORM`. Reading it off the sequence would have given the same answer as a guess.

**And that recovery needed its own fix first.** It takes the name from what was asked for,
so it must pair each output with its input line; refused lines produce no output, so naive
positional pairing shifts everything after the first refusal by one. Into plausible
answers, not obvious ones.

**A heredoc cost twenty minutes by mangling backslashes.** A probe string written as `\n`
reached the script as a real newline, so a match against the file's literal `\n` failed and the
file looked corrupted when it was fine. Known trap in this session already; the Write tool
is the answer and I went back to it.

