# 2026-08-21 - Behavioural provenance (D180)


The knowledge base recorded seventy-two functions' behaviour and could not say how it knew
any of it. `found_by` covers the **name**, and CI re-derives every committed name from this
repository's own inputs; nothing did the same for an arity, a return kind, or an edge case -
which are the facts that change what the emulator does.

**Why now.** The stated goal is an unattended loop where agents read a run's findings and
fix what they name. Facts then arrive by way of something that has read the public internet,
including the other projects in this space, so "this is what the function does" can be
recalled and dressed as reasoning - the convergence problem principle 1 exists to prevent,
with no reading step to point at. Abstinence is unenforceable and unprovable. Accounting is
neither.

`known_by` takes `published`, `measured`, `guest-observed`, `assumed`, with `cites` for the
two that claim outside support and `assumptions` for whatever the entry's own provenance
does not cover. **There is deliberately no value meaning "I already knew it"**, and a test
holds that property: every value must be either citable or probeable, so a hypothetical
`recalled` could not be added without failing it.

Refused rather than defaulted. `learn` rejects an entry that does not account for itself,
because every available default is a lie in one direction or the other.

**The backfill was the interesting part** - 42 `published` (libc genuinely is: ISO C, POSIX
and the Itanium C++ ABI specify these by name), 20 `guest-observed` (vendor interfaces a real
title established by experiment), 10 `assumed` (entries whose purpose is read off the name).
60 open questions, which is now the probe worklist for hardware.

`snprintf_s` is the worked example and the reason provenance is per-claim: the base interface
is ISO C 7.21.6.5 and citable, the bounds-checked variant is Annex K - optional, and both its
truncation return and whether it terminates the buffer are unverified. One field per function
would have had to round that up, and rounding up is how a guess becomes a fact.

Also folded `learn`'s ten arguments into a `Learned` struct - the field list was declared,
destructured and re-passed, which is three places to forget the new one.


