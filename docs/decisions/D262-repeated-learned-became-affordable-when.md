# D262 - Repeated `learned` became affordable when the list shrank, and nobody noticed


**decided** · 2026-08-25 · a consequence of somebody else's change, found by costing it

D195 kept `learned` to one position per pattern because it appears in enumerations whose
size is the product of their parts, and squaring a large list is not affordable. That was
correct and is no longer binding.

`prefix-module-learned-learned-verb-tail` is the shape five of the measured names need:

| `learned` size | candidates |
|---|---|
| 12,255, as it stood this morning | **12,164,987,025,000** |
| 177, after the list was reduced | **2,537,649,000** - about two thirds of the whole current space |

Four thousand eight hundred times cheaper, which turns "never" into "one afternoon". The
reduction was made for unrelated reasons and its most valuable consequence was invisible
until something costed the shapes it unlocked.

**Worth stating as a general point.** A vocabulary list's size is not only a coverage
question, it decides which *shapes* can exist at all - so shrinking a list can buy more
names than growing it. Nothing in the tooling said so, which is why `tests/shapes.rs` now
prints the cost of every shape it asks for.

