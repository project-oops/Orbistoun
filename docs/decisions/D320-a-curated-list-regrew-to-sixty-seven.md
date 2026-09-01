# D320 - A curated list regrew to sixty-seven times its size, and only the clock noticed


**decided** · 2026-08-26 · found by asking why a test had been running for nine minutes

`learned` holds 177 words. It held 11,842, because a `names` run harvested strings from
guest modules and merged everything `is_word` accepts - and two thirds of what it accepts
is not vendor vocabulary at all:

```
Agent5pause  Agent6enable  Agent2gc  Document9terminate  Layer18accumulated
```

Those are Itanium C++ mangling: a length prefix and the identifier it measures. 6,451 of
the 11,845 entries had that shape. They can never appear in a `sce*` export, so every one
was pure cost.

**And the cost is quadratic, because `learned` appears twice in two shapes.** At 177 words
those two are 165 million candidates each. At 11,842 they are 757 billion each, so a
vocabulary round - which re-sweeps every shape using the grown slot at full size (D264) -
went from about 350 million candidates to 1.5 trillion.

The visible symptom was the gate. Six tests in `orbistoun-propose` swept that space on
every run; one of them was still going after nine and a half minutes in *release* and had
to be killed. Nothing failed. Nothing said "the vocabulary grew" or "this round costs four
thousand times what it did yesterday". The tree simply stopped finishing, which reads as a
slow machine rather than a regression.

**The reduction to 177 was a one-off act of curation that nothing held.** D262 costed the
shapes it unlocked and called that its most valuable consequence; D259 restored two words
it had stranded. Neither wrote anything that would stop the next harvest putting the 11,665
back, and the next harvest did.

Three things, and the third is the one that matters:

- The list is back to 177, and `audit --repair` re-derived the 26 records whose indices
  moved. The seven survivors were exactly D259's `sceAjmBatch*` and `sceLibcMspace*`,
  stranded again by the same two missing words - which is where the number 177 comes from,
  and confirmation the restore landed on the right list.
- The tests no longer pay for the quadratic shapes. Dropping a *shape* from a test grammar
  is safe where dropping a *word* is not, because an index is a position inside one
  pattern's own radix (D214), so the records those tests produce are now checked against
  the **complete** shape set - a stronger assertion than the one they replaced. The suite
  went from over fifteen minutes unfinished to 76 seconds.
- **A vocabulary list needs a size the tooling defends.** Curation that lives only in the
  file is undone by the tool that writes the file. `is_word` is the place - a fragment
  whose digits are followed by a lowercase identifier is a mangled symbol, not a word -
  and until that lands, the 177 will regrow the next time anybody harvests strings.

The general shape is one this log keeps finding: a property that matters, held by nobody,
noticed only when something unrelated broke. Here the property was a *number*, and the only
instrument that reported it was wall-clock time on an unrelated test.

