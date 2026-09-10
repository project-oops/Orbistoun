# D655 - The symbol level agrees

**Status:** measured
**Date:** 2026-09-09

## What was compared

D654 left the table level clean and the symbol level unreachable. With every module now getting
past the tables, the differential compares three more things per module:

- **The dynamic symbol count, from each reader's own derivation.** Orbistoun takes it from
  `DT_HASH`'s `nchain`; SELFish divides `symtabsz` by `syment`. Two independent readings of one
  file, which is the only reason comparing a count is worth anything.
- **Every encoded import, joined on symbol index**, comparing the hash and the *resolved* library
  and module names. The index is the join because a relocation names its symbol by index and by
  nothing else.
- **The relocation census by type**, for `rela` and `jmprel`.

**All three agree on all 29 modules.**

```text
differential: 29 module(s), 0 with a disagreement, 28 differing only in units
```

## Three meanings established before a line of comparison was written

D654 cost four rounds to one mistake - two fields sharing a name and not a meaning - so each of
these was read out of the two crates' source first:

- **SELFish's `imports` returns encoded names only.** A plain undefined symbol, what an open
  toolchain emits, is skipped there and carried here as `NameForm::Plain`. Counting both lists
  would report a difference on every module that has one.
- **The library and module come out differently**: orbistoun carries raw ids and resolves them
  through its own tables, SELFish resolves them for you. The names are the comparable thing, and
  the one worth comparing - a wrong id attributes an import to the wrong module and produces a
  name that fits and means nothing, which is D117 and cost a week.
- **The NID is byte-reversed between the two.**

## The byte order is measured, not assumed

This is the one that would have been assumed a fortnight ago. SELFish's answer to
`REQ-20260909T1250Z-1f74` said orbistoun prints these reversed; orbistoun's own `NidHasher::hash`
documents "the first eight bytes of the SHA-1 digest, read little-endian", which reads as though
the two should agree. **Two documents, opposite implications, and picking one would have been the
D654 error again.**

So the test tries both orders, counts which matched, and reports it. Every encoded import in every
module matches only reversed - 583 in PPSA02664's eboot, 1,733 in PPSA21564's - and no module
mixes the two. A module that *did* mix them would mean neither reader has a convention, and that
is a separate reported field rather than a silent majority vote.

## Units are counted apart from disagreements

Every module with encoded imports differs on byte order. Reported as a disagreement that reads as
*the two readers disagree about 28 modules*, when they agree about all of them. `Disagreement`
carries a `units` flag and the headline counts them separately - the line still prints, because a
convention nobody wrote down is how three of D654's four errors happened.

## Two vacuous passes closed

A comparison that never ran and a comparison of nothing both look exactly like agreement, which is
the failure this whole test exists to catch one level down. Two were possible here and both now
report themselves:

- `symbol_count` answers `None` when `symtabsz` or `syment` is zero, which would skip the
  comparison silently. It answered on all 29, and the skip would have said so.
- An empty relocation census compares equal to an empty one. Neither table was empty anywhere,
  and a pair that was would now print rather than pass.

Both were written before knowing they would not fire. That is the point of them.
