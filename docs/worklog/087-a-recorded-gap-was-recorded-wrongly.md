# A recorded gap was recorded wrongly


D105 carried a known gap: the typed-buffer opcode read as three bits where the
specification gives four, described as unobservable because "the family defines eight
opcodes, so the two readings cannot disagree about any instruction that exists and no
fixture can separate them".

Every clause of that is wrong, and the reference for this generation says so.

- **The fourth bit is not adjacent.** The opcode is split: bits 18:16 of the first word and
  one more at bit 53, in the *second*. "Widen it to four bits" would have read a
  neighbouring field as opcode.
- **There are sixteen opcodes.** Eight operations and their eight half-precision variants.
- **They are separable, and are being conflated right now.** Assembling
  `tbuffer_load_format_x` and `tbuffer_load_format_d16_x` gives an **identical first word**;
  the only difference is the bit not read.

It stays a recorded gap - a field cannot be written in two pieces in this table, and the
family is refused in its entirety pending a resource model - but it is measured now, the
missing bit is located, and there is a test pinning the conflation so that closing it fails
loudly rather than leaving a comment describing a fixed problem.

> **Overtaken on 2026-08-21**, and both stated reasons expired. `Encoding` gained an
> `opcode_extension`, so a field *can* be written in two pieces; and the family is no
> longer waiting on a resource model, because that arrived with the untyped accesses. The
> pinned test failed exactly as designed and is kept inverted. See "The split opcode, and a
> test that asked to be deleted" below. Left as written, because a log that gets edited to
> look correct stops being evidence of anything.

247 tests green across the shader-side crates.

### Surprises

- **The guard was already there, for a different reason.** Both variants map to one opcode
  in the name table, which refuses to load when an opcode is named twice. A corpus
  containing a half-precision variant becomes a startup failure naming the collision rather
  than a wrong mnemonic in a report.
- **The queue index described a decision the decision does not contain.** The summary line
  for D105 was about the opcode width; the entry it names is about hand-written fixtures,
  with the width as one paragraph inside it. Worth knowing that the index is a summary
  someone wrote, not a view of the entries.

