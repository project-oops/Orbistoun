# orbistoun-names

Generating and confirming candidate symbol names, so a hash can be turned back into one.

**Models:** the candidate grammar and its vocabularies, indexable pattern enumeration,
threaded search, harvesting a standard-library word list from FreeBSD source, reading
identifier-shaped strings out of a guest module's own bytes, and widening the vocabulary
with whatever a run confirmed.

**Deliberately fakes:** nothing. A name is either confirmed by the hash or it is not
reported.

**Design note.** A NID is a truncated SHA-1 and is not invertible, so there is exactly one
way back: hash names you can think of and see which match. Everything here exists to think
of a great many names cheaply - and a match is **proof**, not a lookup, which is what keeps
the symbol database clean-room. See [PROVENANCE.md](../../docs/PROVENANCE.md).

**Two sources, and only one is guesswork.** Published standards (ISO C, POSIX, and the
FreeBSD-derived library that exports them) are not guesses at all. Vendor naming follows a
regular shape - prefix, module, action, object - so candidates can be enumerated
combinatorially. That is guesswork, but structured, and self-verifying.

**The vocabulary is data, not code** (principle 5). Defaults are embedded so the tool works
out of the box; any file of the same shape replaces them.

**Patterns are indexable rather than iterated.** Each can produce its `n`th name directly,
treating the index as a mixed-radix number over its vocabularies. That is what lets the
search split across threads by range with no shared state - and it makes the generator
testable, because a specific index has a specific answer.

**The titles carry their own names.** Reading identifier-shaped strings out of a module
found 175 names in one pass, where a sweep of 2.58 billion generated candidates found none
(D193). Anything confirmed is split into words and fed back into the grammar, so the search
gets cheaper every time it succeeds (D195).

**Status:** done and in the loop. 636 names in the shipped database.
