# D195 - A run widens the grammar it searched with


**decided** · 2026-08-22

A name read out of a module names one import and stops. Its **parts** are what the generator
was missing, and until they reach the grammar the same gap reappears on the next title: the
search that could not spell `Sema` could not spell it twice (D193).

`names` now writes confirmed words into `vendor.toml` rather than printing them. Data, not
code (principle 5), so nothing rebuilds and nothing needs reviewing before it takes effect.

**Written rather than suggested**, because a suggestion somebody has to act on is a step
that does not happen unattended - and unattended is the whole point.

### Kept apart from the hand-written words

Two reasons and both matter. **Provenance**: a word somebody chose and a word a module
yielded are different claims, and merging them loses that permanently. **Cost**: one pattern
squares the `object` list, so growing it grows the search quadratically. The `learned` list
is used once per pattern, so a word costs modules x verbs candidates rather than squaring
anything.

### Three guards, because a program editing its own inputs needs them

- Filtered against **every** vocabulary list, not just its own. A word the grammar can
  already spell adds candidates that were always reachable.
- **Parsed before it is trusted.** A grammar the next run cannot read would break the search
  rather than widen it, and the failure would arrive one run after its cause.
- **Never fatal.** The names are confirmed and written regardless; failing to widen costs
  the next search, not this one.

Seeded with 175 words derived from names already confirmed in the tracked database -
`Sema`, `Equeue`, `Munmap`, `Setaffinity` and the rest - none of which the grammar could
previously spell.

### Pinned by tests, because the live trigger is rare by design

A run only writes when a title yields a name nothing has seen before, which is exactly the
uncommon case. Verifying it in the wild would mean waiting for an event that may not arrive
for weeks, so the write path is held by tests instead: a new word lands, an existing one
does not duplicate, a word the grammar can already spell is refused, the list is created
when absent, fragments are rejected, and **what is written still parses and still resolves
its patterns**.

That last one is the load-bearing test. Everything else is tidiness; that is the one where
being wrong breaks the next run rather than this one.

