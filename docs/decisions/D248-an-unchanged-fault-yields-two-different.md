# D248 - An unchanged fault yields two different offsets, so it read as disagreement


**decided** · 2026-08-25 · caught by reading the first run of a sweep that had just been built

The return sweep asks: forced to answer a sentinel, does an import's fault land a fixed
distance from it? Twenty-two of twenty-three came back `Inconsistent`, which reads as noise
and is how it was nearly written up.

They were all the same thing, and it is the strongest negative the sweep can produce. An
**unchanged** fault is measured from two *different* planted values, so it yields two
different offsets - the arithmetic reports disagreement when the truth is that the guest was
indifferent to what it was handed. `Agreement::Unchanged` is checked before any subtraction,
because subtraction cannot see it.

The same distinction already existed one level up: `conclude` tracked unchanged faults
separately and produced `Finding::Unmoved` from them. Extracting the shared rule into
`agreement` dropped it, because the argument sweep carried it outside the arithmetic and the
return sweep had nowhere to carry it. A refactor losing a distinction that a comment two
functions away still relied on.

