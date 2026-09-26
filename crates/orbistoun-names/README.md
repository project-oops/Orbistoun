# orbistoun-names

Generating and confirming candidate symbol names, so a hash can be turned back into a name.

It holds the candidate grammar and its vocabularies, indexable pattern enumeration, threaded
search, harvesting a standard-library word list from FreeBSD source, reading
identifier-shaped strings out of a guest module's own bytes, and widening the vocabulary with
whatever a run confirmed. A name is either confirmed by the hash or not reported.
[orbistoun-propose](../orbistoun-propose/) grows its grammar in memory.

## Method

A NID is a truncated SHA-1 and is not invertible, so the only way back is to hash names and
see which match. A match is proof, not a lookup, which is what keeps the symbol database
clean-room. See [docs/PROVENANCE.md](../../docs/PROVENANCE.md).

Candidates come from three sources:

- **Published standards.** ISO C, POSIX and the FreeBSD-derived library that exports them.
  These are not guesses.
- **The module's own strings.** Identifier-shaped strings read out of a guest module are the
  cheapest candidates there are.
- **The grammar.** Vendor naming follows a regular shape - prefix, module, action, object - so
  candidates are enumerated combinatorially. Guesswork, but structured and self-verifying.

Anything confirmed is split into words and fed back into the grammar, so each success makes
the next search cheaper.

## Rules

- **The vocabulary is data, not code.** Defaults are embedded so the tool works out of the box;
  any file of the same shape replaces them.
- **Patterns are indexable.** Each pattern produces its `n`th name directly, treating the index
  as a mixed-radix number over its vocabularies. The search splits across threads by range
  with no shared state, and a specific index has a specific, testable answer.
