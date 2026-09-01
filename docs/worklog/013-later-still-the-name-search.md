# 2026-08-19 (later still) - The name search


Phase 2's missing half. `orbistoun-names`: a grammar, an indexable generator, a parallel
solver, and the `names` command. D068-D069; 24 crates, 253 tests.

Fully clean-room, confirmed as the preferred approach by the user: nothing is consulted,
names are proposed and the hash confirms or rejects each one.

**Measured: 26 million candidates per second**, 251 million in 9.7 seconds on sixteen
threads. A billion in about forty.

### Surprises

- **The obvious generator design is a hundred times too slow.** An iterator yielding
  `String` allocates once per candidate, so the allocator - not SHA-1 - sets the pace.
  Patterns now write into a per-thread buffer and the hasher takes bytes.
- **Indexable patterns solved two problems at once.** Reading the index as a mixed-radix
  number over the word lists makes threads independent *and* makes the generator
  testable, because a specific index has a specific answer.
- **A ceiling on parts needed refusing, not truncating.** Decoding into a fixed stack
  array meant a grammar with too many parts would index out of bounds. Caught while
  writing it, but it is the exact shape of bug that would survive review.
- **A wrong suffix and an inadequate vocabulary look identical** - both report zero. The
  published standard-library names separate them: they are fixed by standards, so if a
  module links a C library and none of ~470 match, the names are not what is wrong.

### Outstanding

**The search cannot name anything without the hash suffix.** It is a runtime input and
deliberately absent from the tree (D006), so this is now blocked on the user supplying
`--suffix-hex`. Everything else is built and verified: 251 million candidates tried
against a real 733-import executable, plumbing confirmed, zero named as a correct
suffix would be required to change.

