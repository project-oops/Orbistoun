# D091 - Register extraction is mechanism; the shader-address map is a hypothesis

**assumed** · 2026-08-19 · shape invariant pinned 2026-08-21 (the numbers stay a hypothesis)

`orbistoun-gpu::registers` pulls register writes out of a walked submission, then uses
`data/packets.toml` to guess which of them hold shader addresses.

Those two things have very different confidence and are deliberately separated.

Pulling out register writes is **structural** - a type-0 packet writes consecutive
registers from a base in its header, a `SET_*_REG` packet from an index in its first
body word - and stays correct whatever any register means.

Deciding that a particular register holds the low half of a fragment shader's address
is a **guess**, and unlike the encoding table there is no reference disassembler to
check it against. So it lives in data, and what comes out is a `ShaderCandidate`
rather than an address. Nothing follows one blindly.

Getting that separation right matters more than getting the guess right: the mechanism
will still be correct after the table is corrected.

### One checkable thing inside the guess

The register *numbers* remain a hypothesis with no oracle - the instruction-set reference
names the registers a shader's start address comes from without giving their offsets, so
there is nothing to check the numbers against and transcription is all there is.

It does say something checkable about their **shape**, though: the address comes from a
`LO`/`HI` pair, per stage, and the halves are consecutive. So every stage in the table must
have exactly one of each half, with the high sitting immediately above the low.

That is worth a test because it catches the class of mistake this table is most exposed
to. It was transcribed by hand from a document, and hand transcription fails by
transposing a digit or dropping a line - both of which break the pairing and neither of
which is visible by reading the file back, because a wrong-but-plausible register number
looks exactly like a right one.

It does not make the numbers right. It makes one way of being wrong loud, which is the
most that is available until something with a real submission can check them.

Two smaller choices inside it. A half whose value is neither `low` nor `high` is
**refused** rather than defaulted - defaulting would swap the halves of every address
it touched, producing plausible values wrong by four billion. And a stage with only one
half seen produces **no candidate**, because an address missing its high word points
into the bottom four gigabytes and reads as an ordinary low address rather than as a
fault.

