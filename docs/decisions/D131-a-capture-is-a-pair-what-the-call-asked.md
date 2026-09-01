# D131 - A capture is a pair: what the call asked for, and the bytes it appended


**Status:** assumed

`crates/orbistoun-gpu/tests/captures/` takes `<name>.toml` plus `<name>.bin`, and
`tests/vocabulary.rs` checks every expectation in the first against a decode of the
second.

**Why a pair rather than a command buffer.** A recorded buffer on its own would have to
be read *through* `data/packets.toml` - the table under test - so agreement would prove
nothing. The guest's graphics layer builds buffers through library calls before
submitting them, so a call's arguments state the answer while its appended bytes are the
question. That is what makes this an oracle rather than a mirror.

This is the one layer of the GPU subsystem checked against nothing. Everything above it
has an external reference: instruction decode against a disassembler, translation against
a real GPU. A wrong register base attributes every write in its class to the wrong
register *consistently*, and a wrong shader-address row means shaders are sought in the
wrong place - neither produces an error, both produce a submission that yields nothing
and looks like an unremarkable frame.

**An empty corpus reports rather than passes**, and `orbistoun.sh check` surfaces it
beside the device-test skip. Same rule: "nothing to check" and "everything checks out"
must never look the same.

**The comparator returns errors rather than panicking**, so both directions are tested
today with no captures at all. A comparator only ever exercised by data that makes it
pass is indistinguishable from one that returns success unconditionally - and this file
exists entirely to fail when the table is wrong.

