# The execution mask becomes real


The wavefront model has had masked writes since it was built - every vector write and
every store selecting on the lane's bit - and none of it had ever been exercised, because
no instruction could change the mask. It was all-ones in every shader that translated,
so every select chose the new value every time: correct, and untested.

That is now closed. `s_mov_b64 exec, …` writes the mask, and `s_and_b64`, `s_or_b64` and
`s_andn2_b64` do the arithmetic a guest actually performs on it - narrow on entering a
conditional region, widen on leaving, and `andn2` for the lanes an if-branch did not
take. Thirty-two execution tests, all on a real device, including the case that separates
`andn2` from a plain `and`: remove lane 1 and lane 0 must still write. Translating it as
an `and` inverts the sense of every else-branch, and only that direction catches it.

**The lane model refuses a shader that writes the mask**, rather than ignoring it. It has
one invocation per lane and no way to represent an inactive one, so ignoring the write
would run every lane regardless - plausible output, wrong answer, nothing to point at.

**That refusal made `Fidelity::Auto` wrong, which was the useful part.** Auto resolved to
the lane model unconditionally, on reasoning written into the function itself: any shader
needing more would contain an instruction the translator refused anyway, so the wrong
level could not be chosen - "not by analysis, but because the translator stops first". The
comment went on to say this was safety by accident and would stop holding the moment
those instructions were implemented. It stopped holding in the same commit that
implemented them. Auto now inspects the shader and picks the model it needs.

Worth noting how that went: the note predicted its own expiry precisely, and the expiry
still arrived as a surprise mid-change rather than as something planned for. **A comment
saying "this will break when X happens" does not fire when X happens.** The test that
now asserts Auto's choice does.

**Surprises.**

- **Three opcodes of one family solved three different field widths.** `s_and_b64`,
  `s_or_b64` and `s_andn2_b64` each came out too narrow in whichever source slot happened
  never to receive a value above 127 - and it was a *different* slot each time, so
  reading down a column would not have shown it. A source field is a property of the
  encoding, so opcodes of one family reading the same bits at different widths cannot all
  be right.

  This has now happened four times across three sittings, and every time it was caught by
  a person putting rows side by side. That is a habit, not a check. The solver now
  compares field widths within a family and names any disagreement, with the cure -
  give that field its own high sample. Verified by removing the fixing probes and
  confirming it reports both, rather than by trusting that it would.

- **`s_andn2_b64` has no single SPIR-V opcode.** It is a complement then an and, and
  there is no "and not". Reaching for the nearest single instruction would have been a
  plain `and`, which is right in exactly the cases a first test would cover.

**Not done.** Branching. The mask can be computed and honoured, but a jump taken when no
lane survives it cannot be expressed yet - SPIR-V demands structured control flow and the
guest's is implied. That is the remaining substance of D098 and it is a design problem
rather than an implementation one. `Fidelity::Subgroup` is still a stub. Comparison
instructions do not translate, so the tests above use constants where a real shader would
use a comparison result.

