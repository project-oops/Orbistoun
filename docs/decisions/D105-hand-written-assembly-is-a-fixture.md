# D105 - Hand-written assembly is a fixture source, and it caught a wrong row


**Status:** decided (2026-08-21) - the gap it recorded was described wrongly, then closed

`orbistoun-gen fixtures` now accepts `.s` sources alongside `.ll`, assembling them
with `llvm-mc` instead of compiling with `llc`. The disassembly step afterwards is
identical, so a hand-written fixture is verified exactly as a compiled one is.

It exists because SOPK, MTBUF and VINTRP could not be reached any other way. They arise
from specific constant patterns, typed buffer formats and fragment interpolation, and no
LLVM IR the generator can write makes the compiler emit them - so those three rows stayed
transcribed-and-never-checked while the other fourteen had been verified against a
reference (D085, D089).

**A hand-written fixture is the weaker kind and the table now says so.** The instruction
is one somebody thought of rather than one a compiler reached for, so it shows the row is
right about the instructions written, not that it covers everything the family can
express. Compiled fixtures stay the default for exactly that reason.

**It found a wrong row on the first run.** VINTRP's encoding value was this family's
value from the first two generations of the architecture, while the table targets a later
one - so the decoder called `v_interp_p1_f32` unrecognised the moment a fixture reached
the family. The row had been sitting next to a comment saying it was unverified, which is
a different thing from being verified as wrong. Corrected from the published
documentation for this generation; the reference detected the error and did not supply the
fix, which is the line D085 draws.

**One gap left recorded rather than closed.** MTBUF's opcode field is three bits in this
table and four in the specification.

### That gap was recorded wrongly, and it is bigger than it said

The original wording claimed the family defines eight opcodes, so the two readings could
not disagree about any instruction that exists and no fixture could separate them. Both
halves are wrong, and the reference for this generation says so plainly.

**The fourth bit is not adjacent.** The opcode is *split*: bits 18:16 of the first word and
one more at bit 53, which is in the **second** word. So "widen it to four bits" would not
have been the fix - bit 19 is not the opcode, and widening would have read a neighbouring
field as part of it.

**There are sixteen opcodes, not eight.** Eight operations and their eight half-precision
variants. The ninth opcode is not something to meet later; it exists now.

**They are separable, and currently conflated.** Assembling `tbuffer_load_format_x` and
`tbuffer_load_format_d16_x` produces an **identical first word** - the only difference is
the bit this table does not read. So every half-precision variant decodes today as the
operation it is a variant of.

### Closed on 2026-08-21

The stated reason for leaving it open was that a field cannot be described in two pieces
in this table, and that inventing a schema for one family with no consumer would be
building for a case that does not exist.

Both halves of that expired. `Encoding` now carries an optional `opcode_extension`, which
is one field rather than a general mechanism for arbitrary splits - every split this
instruction set actually has is a single continuation, and a list of pieces would be a
shape to maintain for a case that has never occurred. And MTBUF is no longer refused
pending a resource model: the resource model landed with MUBUF, so a typed buffer access
is the same descriptor and the same addressing with a format conversion on top.

The pinned test did its job. `the_typed_buffer_variants_are_known_to_be_conflated` was
written as a *passing* assertion that the conflation existed, so that closing the gap
would fail and say which notes to update. It failed with exactly that message. It is kept,
inverted, as `the_typed_buffer_half_precision_variants_decode_distinctly` - and it asserts
the variant is its counterpart **plus eight**, not merely different, because a continuation
shifted to the wrong place still produces two distinct numbers.

**The generators duplicate this rule.** `classify()` exists in
`orbistoun-gen fixtures` and `orbistoun-gen operands` as well as in the decoder. Leaving
those reading only the contiguous part would have classified a half-precision variant as
its counterpart and emitted a second name for an opcode that already had one - which the
name table refuses to load, so that particular drift would have surfaced loudly. Not every
drift would, which is why they were changed together rather than when something broke.

**The half-precision names are still unknown, deliberately.** `mnemonics.toml` is generated
from what a compiler actually emitted and a reference disassembler actually named, and no
fixture contains a `d16` variant yet. So one now decodes as a known family with an
unrecognised opcode - which is honest - rather than as the wrong instruction, which is what
it did before.

What changed is that it is now *measured* rather than guessed at, and the entry says where
the bit is so the fix is an edit rather than an investigation.

**And it fails loudly rather than quietly.** Both names map to one opcode in the name
table, which refuses to load when an opcode is named twice. A corpus containing a
half-precision variant turns this into a startup failure naming the collision, not a wrong
mnemonic in a report. That guard was already there for a different reason and covers this
one for free.

**The fixture list was hardcoded, so adding a file changed nothing.** `unreached.s`
generated its `.bin` and `.txt` and the suite reported green while never opening either -
the same shape as a device test skipping quietly. There is now a test asserting the
fixtures on disk and the fixtures the suite reads are the same set.


