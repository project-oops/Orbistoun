# D214 - Proposers pair with an oracle and ask for words

**Status:** decided
**Date:** 2026-08-24

`orbistoun-propose` joins a proposer to an oracle, and no proposer is built before its oracle.
The vocabulary proposer asks a model for words for a named grammar position, never for a name;
it is never shown a hash, and a word must be one short capitalised token. A round returns what it
found and discarded and writes nothing.

**Why:** a hash collision is proof, so an invented word costs nothing. A name confirmed through
the word route is `generated` at a pattern and index, re-derivable by the audit; a name proposed
whole could only say "something suggested this". Filtering patterns keeps recorded indices
valid, where narrowing a slot renumbers them.

**Rejected:**
- Asking for whole names: a provenance category nothing can re-derive.
- A proposer for implementations: no oracle.
- Narrowing the sweep to only the new words: invalidates the recorded index.
