# D663 - The codenames are the vocabulary

**Status:** measured
**Date:** 2026-09-10

## §2 was being strained, not satisfied

Principle 2 keeps vendor trademarks out of this project's prose and its own API. `Machine::describe`
answered `ps5/cex/base`, and every run printed it. That is a trademark, in orbistoun's own output,
on every line of every report.

The codenames are not trademarks. `prospero/cex/base` says the same thing and satisfies the rule
the previous wording strained, which is why the vocabulary changes rather than the wording being
softened. obSCEne already speaks it - *"PlayStation OS (Prospero / Orbis)"*, `neo-mode` - so the
two projects now name the same machine the same way, and the sibling's usage was the oracle rather
than a preference of mine.

## Four machines, already expressible

I warned earlier that this was a three-way split against a two-way enum and therefore a design
change. **That was wrong.** `Generation × Revision` already picks exactly one of four:

| generation | revision | codename |
|---|---|---|
| Orbis | base | `orbis` |
| Orbis | pro | `neo` |
| Prospero | base | `prospero` |
| Prospero | pro | `trinity` |

So `Platform` is **derived**, not a third field. A third field would let a `Machine` claim to be a
base Prospero *and* a Trinity, which describes nothing - the same reasoning that makes `Kind` an
enum rather than three booleans.

## Written test-first, and it caught the thing it was written for

The test asserts all four pairs, collects the names, deduplicates, and asserts four remain. The
failure it guards against is two combinations collapsing onto one label - a report saying
`prospero` for a machine that is not one, with nothing to notice. Exhaustive because four is small
enough that sampling would be a choice rather than a constraint.

The second test asserts the negative directly: `describe()` contains neither `ps5` nor `ps4`. A
positive assertion on `prospero/cex/base` alone would pass a hypothetical
`prospero-ps5/cex/base`, and the point is the absence.

Both failed first, for the right reasons - `no variant named Prospero`, `unresolved import
Platform`, `no method named platform`.

## Two existing tests failed, correctly

`the_default_is_what_every_measurement_was_taken_against` and
`the_revision_is_independent_of_the_kind` pinned `ps5/cex/base` and `ps5/dex/pro`. They failed
because the behaviour they pin intentionally changed, and they were updated rather than deleted -
they still pin the same properties, in the new vocabulary.

Nothing else in the tree spoke the old words: a workspace build after the rename was clean, and a
search for `"ps4"`/`"ps5"` outside the machine module found nothing.
