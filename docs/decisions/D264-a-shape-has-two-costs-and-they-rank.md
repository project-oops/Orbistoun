# D264 - A shape has two costs, and they rank differently


**decided** · 2026-08-25 · a pattern added and removed the same afternoon

`tests/shapes.rs` ranks missing shapes by their share of the **whole** candidate space,
because that is what a full sweep pays. By that measure the three cheapest were +0%, +4% and
+11%, and all three were added.

A vocabulary round pays a different price. It grows one slot by the word it is testing and
re-sweeps **every pattern using that slot, at full size** - so what a round costs is driven
by how often the grown slot appears in a pattern, not by that pattern's share of the whole.
`prefix-learned-verb-learned-learned` takes `learned` three times:

| | share of the whole sweep | share of a vocabulary round |
|---|---|---|
| `prefix-learned-verb` | +0% | 12,816 - nothing |
| `prefix-module-learned-learned-verb` | +4% | 171,093,600 - 28% |
| `prefix-learned-verb-learned-learned` | +11% | **406,062,144 - 67%** |

The third is the cheapest-but-one by the first measure and by far the most expensive by the
second. It found **no names**, so it was removed: two thirds of every round's cost, for
nothing. `a_round_sweeps_only_the_shapes_that_use_the_new_words` caught it, by asserting a
round sweeps under a tenth of the space - which it had quietly stopped doing at 13.7%.

**The general point.** "How expensive is this shape" has no single answer, and the ranking
flips depending on which search is running. A pattern is cheap to a full sweep and dear to a
vocabulary round exactly when it repeats the slot that rounds grow, which is the case worth
noticing because it is the one where intuition from the other measure is actively wrong.

The other two stay: 5 names found, reachability over the measured sample up from 28 to 43.

