# D182 - The compatibility record is the other half of the title file


**decided** · 2026-08-21

`Layer::Repo` has always existed in `orbistoun-overrides`, documented as "our shipped
compatibility knowledge", and nothing ever wrote it. The type system described a
compatibility database that did not exist.

A title file said what orbistoun **sets** for a title. Nothing said what orbistoun **got**.

### One file, because two would disagree within a week

The measured half lives in the same file as the settings half. They are keyed by the same
title, edited in the same session, and produced by the same run; two files would
immediately disagree about which was current, and nothing could say which to believe.

**Deliberately not merged.** `Resolved::merge` layers settings per key, which is right for
configuration and meaningless for a measurement - there is no sense in which a user's run
"overrides" the repository's recorded one. They are facts about two different runs, and
*comparing* them is the useful operation. `merge` iterates settings and compat explicitly,
so the new field is excluded by construction rather than by remembering.

### Derived, never typed

Every field comes off the trace. A compatibility database whose grades are hand-written
drifts the moment somebody is optimistic, and the drift is invisible because there is
nothing to check a hand-written grade against.

That also decided the vocabulary. "Playable", "in-game", "intro" would be aspirational
fiction here; every rung is a phase the loader already distinguishes, so a grade is a
transcription rather than an opinion.

### The record is protected from the reward hack

`Status::beats` refuses to rank two results produced under different stub policies, and
`compat record` refuses outright to record a run where unimplemented functions reported
success - **including the first one**. That last clause was found by recording an entry: a
contaminated first entry has nothing to be compared against, becomes the baseline, and then
no honest run can ever beat it. A database carrying a best-ever nobody can reproduce is
worse than an empty one.

The refusal is symmetric. It says "these cannot be compared", not "the bigger number wins",
so a contaminated entry can be displaced by an honest one rather than only by another
contaminated one.

### The ladder stops at `Entered`, and the corpus is why

A `Sustained` rung for "ran to the limit without faulting" seemed obviously worth having.
Populating the record disproved it in one table:

```
PPSA04263-app0   sustained    4 imports 91455278 calls   ran to the time limit
PPSA28061-app0   entered     47 imports      933 calls   image+0x43c4
```

Ninety-one million calls of four functions sorted **above** the run that reached
forty-seven imports - the least informative result in the corpus at the top of the
frontier list. Surviving is an *outcome*, not a distance, and it was already recorded as
one. This is the same argument that had already been accepted for a `Stopped` rung and then
not applied here; the corpus caught what the reasoning missed.

Within `Entered`, distance is distinct imports and then calls. Calls rank last because a
guest spinning on one unimplemented function accumulates them without learning anything -
which is precisely what PPSA04263 does.

### What it says today

| title | reach | imports | calls | standing | outcome |
|---|---|---|---|---|---|
| PPSA28061 | entered | 47 | 933 | 85% | `image+0x43c4` |
| PPSA02664 | entered | 19 | 53 | 53% | the guest called abort |
| PPSA03416 | entered | 19 | 53 | 53% | the guest called abort |
| PPSA25872 | entered | 14 | 1735 | 100% | `image+0x7b591e` |
| PPSA21564 | entered | 13 | 131 | 91% | `image+0x70a932c` |
| PPSA04263 | entered | 4 | 91455278 | 100% | ran to the time limit |

The twin abort at exactly 19 imports and 53 calls is visible at a glance, which no report
about a single title could show.

### Titles cannot travel; everything about them can

`titles/` holds guest material and is never tracked. `compat/` holds identifiers, settings
and numbers, and is always tracked. That split is what lets a finding travel when the title
cannot - a contributor's result is checkable against yours without either of you having the
other's material, provided the entry carries the build and the policy that produced it.
Which is why it does.

