# D261 - Shapes are the binding constraint, not vocabulary, by three to one


**decided** · 2026-08-25 · measured against names the grammar did not find

`audit` says the grammar can re-derive every name in the database, which sounds like full
coverage and is not. Most of those names were *found* by the grammar, so it can necessarily
spell them - measuring coverage against them measures the search that produced them.

`tests/shapes.rs` measures against the names found **without** the grammar - read out of
module strings, or seen in a trace. Those owe nothing to the pattern list, so they are a
sample of what vendor identifiers actually look like. Of 183 such names:

| | |
|---|---|
| reachable under the current pattern list | **28 (15%)** |
| unreachable, needing a shape | **121 (66%)** |
| not splittable into known words at all | 34 (19%) |

So the pattern list is short by more than three times what the vocabulary is short by, and
buying more words is spending on the weaker lever. That confirms, with numbers, a prediction
made independently from the naming side: a word only pays in the shapes that exist, and
`learned` appears in two patterns, once each.

**The first cut of this was wrong and the correction matters.** Counting every
independently-found name gave a 68% vocabulary gap - but 276 of those 464 names are POSIX or
libc, which every vendor pattern begins with the `sce` prefix and cannot spell, and is not
meant to. Scoped to vendor-shaped names the ratio inverts.

**Three shapes are nearly free and would spell fourteen of them today:**

| names | cost | shape |
|---|---|---|
| 3 | +0% | `prefix + learned + verb` |
| 5 | +4% | `prefix + module + learned + learned + verb` |
| 6 | +11% | `prefix + learned + verb + learned + learned` |

The rest are not worth having at any vocabulary size - the ranked list runs to +789%,
+1862% and, for one ten-part shape, five orders of magnitude beyond the whole current space.
A name needing that has to come from somewhere other than the generator.

