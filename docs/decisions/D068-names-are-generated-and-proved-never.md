# D068 - Names are generated and proved, never obtained

**decided** · 2026-08-19 · confirmed by the user

A NID is a truncated SHA-1 and is not invertible, so there is exactly one way back:
propose a name, hash it, and see. Two sources feed the proposals, and only one is
guesswork.

**Published standards are not guesses.** The target C library is FreeBSD-derived, so a
large part of it is ISO C and POSIX under the names those standards fix. Around 470 of
them ship with the tool. This is precisely the lawful reference principle 1 points at.

**Everything else is enumerated.** Vendor names follow a strict shape - prefix, module,
action, object, revision mark - so the plausible space is tiny next to the space of all
strings, and small enough to exhaust. Currently **251 million** candidates across five
shapes.

**No database is consulted, bundled, or downloaded.** Public NID databases exist and
`SYMBOLS.md` already notes that any file of the right shape will load, but nothing here
depends on one. That leaves zero provenance questions attached to a name this tool
produces: it was proposed by a grammar in this repository and confirmed by arithmetic.

**A match is proof; a miss proves nothing.** The hash agrees or it does not, so a
reported name is correct rather than probable. A miss says only "not among those tried",
which is why the vocabulary is data (principle 5) - extending it *is* the method, and it
must never require a rebuild.

**Two things had to be right for this to be usable at scale.**

- *No allocation on the hot path.* Building a `String` per candidate makes the allocator
  decide how long a billion-name search takes. Patterns write into a buffer the thread
  reuses, and `NidHasher` takes bytes.
- *Indexable patterns rather than iterators.* A pattern produces its `n`th name directly,
  by reading the index as a mixed-radix number over its word lists. Threads then take
  disjoint integer ranges, share nothing, and produce a result that does not depend on
  how many of them ran.

**Measured: 26 million candidates per second** on sixteen threads - 251 million in under
ten seconds, a billion in about forty.

