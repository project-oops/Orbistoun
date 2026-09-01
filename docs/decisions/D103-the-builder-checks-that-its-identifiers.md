# D103 - The builder checks that its identifiers resolve


**Status:** decided (2026-08-21) - it has since paid out on new work

`Builder::check` verifies three properties before a module leaves the crate: every
identifier used is defined somewhere, the declarations and function sections define
before they use, and nothing is defined twice. `translate` calls it and turns a failure
into `TranslateError::MalformedModule`.

The case that prompted it: an identifier was reserved for an array's length and the
`OpConstant` defining it was never emitted. Every instruction was well-formed. The
module meant nothing, and the driver did not say so - it faulted, and the fault arrived
as `STATUS_ACCESS_VIOLATION` inside the graphics driver with no indication of which
identifier or which instruction was responsible. That is the second driver fault in this
subsystem whose diagnosis came from `spirv-val` in a virtual machine; the first was an
access chain given one index where the buffer's shape needs two.

### It has since caught one on its own

The second look this asked for is answered by use rather than by argument. Adding the
subgroup level meant four new opcodes, and one of them - `OpTypeVector` - was given a
shape entry listing its own result as one of its uses. The check reported it used before
it was defined, naming the identifier and the opcode, **before any driver saw the module**.

That is exactly the failure it was written for, arriving from a different direction, and it
was diagnosed from the message rather than from a virtual machine. Narrow was the right
size.

The builder hands out every identifier, so it is the one place that can say which were
never given a meaning. Doing it here turns a driver fault into a named error, which is
the same trade `finish` already makes for the identifier bound.

**Deliberately not a validator.** `spirv-val` exists, is authoritative, and a
second-guessing reimplementation would be worse than silence. This checks the three
properties a builder is uniquely placed to check and skips any opcode absent from its
shape table rather than guessing - guessing would mean reading a literal as an
identifier and complaining confidently about a module that is fine.

The shape table is data rather than a match, listing for each opcode where its result
sits and which operands name others. Expressed as match arms it tripped
`clippy::match_same_arms`, and the lint was right: the same four facts about each
opcode, with nothing computed, is a table.

