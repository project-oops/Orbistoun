# Three assumptions asked a question they had only ever answered themselves


The review queue's three workable entries, closed the same way each time: find the thing
the entry *asserted* and make something check it.

**D112's read window.** The entry called 64 KiB "a guess with no real shader to check it
against". There are shaders now and the largest is 320 bytes, so the number is generous by
two orders of magnitude - but that was the less useful half. The half that mattered was
untested: what happens when a shader *doesn't* fit. Truncating is the plausible failure,
because a real shader cut at the window decodes cleanly right up to the cut and produces a
module that is a genuine prefix of the correct one. Nothing about it reads as wrong. The
refusal existed; no test had ever taken that branch.

**D091's register map.** The numbers stay a hypothesis - the reference names the registers
without giving offsets, so there is nothing to check them against. It does constrain their
*shape*: an address is a consecutive `LO`/`HI` pair, per stage. That is worth a test
because the table was hand-transcribed, and hand transcription fails by transposing a digit
or dropping a line, neither of which is visible by reading the file back.

**D108's implicit operands.** The entry justified telling "carries no bits" apart from "the
probes never varied it" by asserting the assembler refuses anything else there. Never
checked - prose, in exactly the way D128's width reconciliation was prose, and D128 turned
out to be wrong. So it asks now: substitute a value, assemble, compare words.

### Surprises

**The prose had missed an outcome, and it was the dangerous one.** "Refuses / does not
refuse" is two cases; assembling gives three. Accepted-with-different-words means a field
exists and the probes missed it, and the old rule would have filed that as implicit - an
operand recorded as un-encoded while the encoding carries it, decoding forever as whatever
the sample happened to say. The solver now refuses to solve, same as every other operand it
cannot explain.

**The measurement came back stronger than the claim.** `v_cmp_lt_f32_e32 vcc_lo, v0, v1`
is *accepted* and encodes to `0x7c020300` - bit-identical to the `vcc` spelling. So the
evidence is not "nothing else is legal there", which would still allow a field with one
legal value. A different spelling changed no bit. It also settles the two-spellings note in
D108 from the other side: the disagreement is about naming, not about which register.

**I diffed the wrong file and briefly believed nothing had changed.** `operands.toml` is
not the generator's output; `opcode-operands.toml` is. The first check compared a file
nothing writes and reported UNCHANGED, which was true and meaningless. The four implicit
rows do survive - but for a few minutes that was luck rather than knowledge.

**Two failures in the gate that predate this work.** A broken intra-doc link in
`pipeline.rs` pointing at a `Queue::registered` that does not exist, and cargo-machete on
`orbistoun-gui` and `orbistoun-worker`. Fixed the first; the other two crates are not this
thread's.

