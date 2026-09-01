# The last three encoding families, and one of them was wrong


All seventeen encoding families are now verified against a reference. The three that
were not - SOPK, MTBUF and VINTRP - are reached by hand-written assembly rather than
compiled shaders, because no LLVM IR the generator can write makes the compiler emit
them. `orbistoun-gen fixtures` takes `.s` sources through `llvm-mc` and the
disassembly step afterwards is identical, so those fixtures are verified exactly as the
compiled ones are. 126 instructions across 10 fixtures, every boundary matching.

**VINTRP's row was wrong.** Not unverified - wrong. It carried the family's encoding
value from the first two generations of the architecture, while this table targets a
later one, and the decoder called `v_interp_p1_f32` unrecognised the moment a fixture
reached the family. It had been sitting for weeks next to a comment saying it was
unverified.

That comment is the thing worth dwelling on. It was accurate, it was prominent, and it
changed nothing - because "unverified" reads as "probably fine, low priority" and
"wrong" reads as "fix this now", and there was no way to tell which it was without doing
the work. **A caveat is not a substitute for a check; it is a note that no check exists.**
The fix, once the fixture existed, took two minutes.

**Adding the fixture changed nothing until it was listed.** The generator produced
`unreached.bin` and `unreached.txt`, the suite ran, and it reported green - because the
fixture list in the test is written by hand and the new file was not in it. The same
shape as a device test skipping quietly: no failure, no warning, just silence where
coverage was assumed. There is now a test asserting the fixtures on disk and the
fixtures the suite reads are the same set, which is the only version of this that
survives someone forgetting.

**Surprises.**

- **The backlog entry had already worked out the approach.** It said reaching these three
  probably meant hand-written assembly through `llvm-mc`, since the disassembly step
  afterwards is identical. That is exactly what this is. Written months of work earlier
  by someone with no more information than is available now - which says the note was
  worth writing even though nobody acted on it at the time.

- **MTBUF's syntax was rejected twice before it assembled.** The format specifier goes
  before the addressing modifier, not after, and the older separate `dfmt:`/`nfmt:`
  spelling is not accepted on this target at all. Both were refusals rather than silent
  acceptance meaning something else, which is the good failure - the alternative is a
  fixture that assembles into an instruction nobody asked for.

- **A gap was left open on purpose.** MTBUF's opcode field is three bits here and four in
  the specification. The family defines eight opcodes, so the two readings cannot
  disagree about any instruction that exists and no fixture can separate them. Widening
  it would be a change no evidence supports. Recorded in the table and in the review
  index rather than quietly corrected, because a change made on no evidence is
  indistinguishable from a change made on bad evidence six months later.

**Not done.** `Fidelity::Subgroup` is still a stub, the Lane model is still unmasked, and
control flow with the execution mask - the largest unbuilt part of D098 - remains the
next substantial piece. Operand layouts are still solved for fifteen opcodes only; the
three newly-verified families have none, which is a separate question from their
encodings being right.

