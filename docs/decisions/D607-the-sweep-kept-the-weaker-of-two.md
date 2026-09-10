# D607 - The sweep kept the weaker of two true records

**Status:** measured
**Date:** 2026-09-08

## How it surfaced

Adding a console's export table to the target set (D605) named eighty-nine hashes at once, and
the knowledge base immediately failed with forty disagreements of this shape:

```text
atoll: found_by = published-standard but symbols/generated.json re-derives it as static
isalpha: found_by = published-standard but symbols/generated.json re-derives it as static
sceKernelClose: found_by = generated but symbols/generated.json re-derives it as static
```

Both sides were telling the truth. `atoll` **is** a published C name and it **also** appears in
a module's bytes, so two sources named it in the same run. The question is which record the
database keeps, and it was keeping the wrong one.

## The comment was wrong about its own code

```rust
// Keep the first, which is the cheapest to reproduce,
// because sources run in that order deliberately.
found.sort_by(|a, b| a.name.cmp(&b.name));
found.dedup_by(|a, b| a.name == b.name);
```

Sources run **module strings, argument dumps, published names, generated, affixed**. A stable
sort by name keeps insertion order, so the first is the string harvest - which is
`Reproducible::FromModule`, and the published list is `Reproducible::FromRepository`. The order
is not cheapest-first, and the comment asserting it was is the reason nobody looked.

Only `FromRepository` is mechanically checkable, so every one of those names was recorded at a
tier CI can never re-run, when the same run held a record CI could re-run every commit.

**Nothing was ever false.** The static record names a real module that really contains the
string. What was lost is the stronger claim, silently, in favour of a weaker one that reads
identically - which is the failure principle 3 describes, arriving from the other direction:
not reporting more than the measurement supports, but recording less.

## The change

`Reproducible::rank` orders the tiers by what a reader has to have, and the dedup sorts by
`(name, rank)`. The strongest surviving record wins whatever order the sources ran in, so the
ordering of the sweep stops being load-bearing.

Re-run from the same starting point:

| | before | after |
|---|--:|--:|
| named by `standard.txt` | 10 | **29** |
| named by `cross-module` | 60 | **9** |
| re-derived from this repository | 630 | **708** |

Seventy-eight records moved from "ours, but not from this repository alone" to "CI rechecks this
every commit", with no new search and no new evidence.

## Six that stayed, and why they are not the same case

After the fix six entries still disagreed, and they resolve the other way:

- Five `sceKernel*` names the knowledge base recorded as `generated`. The generative sweep ran
  over 3.9 billion candidates **with those hashes in its target set** and did not produce them,
  so the current grammar cannot spell them. `static` is what this run supports; the `generated`
  claim was written when nothing could check it, because the names were not in the database at
  all.
- `snprintf_s`, recorded as `generated`, which is now `affixed` - `snprintf` plus `_s` (D606).

Their `found_by` fields are corrected to what the evidence says. The direction matters: the
database's record is written by the code that did the work, and the knowledge file's field is a
second copy of that claim (D213). Where they differ, the copy is what changes.
