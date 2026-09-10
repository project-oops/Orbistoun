# D624 - The differential counted items and called them findings

**Status:** measured
**Date:** 2026-09-08

## One sentence, ninety-nine times

`probe --against` said this, and it was true and useless:

```text
115 passed there and failed here - each is a defect with its own words
```

Ninety-nine of the hundred and fifteen were `900-surface/corpus_NNNN_libSceSomething` reporting
**none of this library is present**. That is one fact about this build - orbistoun stubs a few
dozen libraries and the console has three hundred - printed ninety-nine times, above the sixteen
entries that each say something different.

"Each is a defect with its own words" was a claim about sixteen of them.

## Grouped by what was said, smallest group first

```text
115 passed there and failed here, saying 17 distinct thing(s) -
a sentence repeated across a family is one finding
  pass -> fail  020-memory/allocate            allocation was refused
  …fifteen more, one line each…
  pass -> fail  x99  none of this library is present
                     900-surface/audioout, 900-surface/corpus_0010_libSceAmpr,
                     900-surface/corpus_0019_libSceAt9Enc, and 96 more
```

Groups are ordered **smallest first inside each transition class**. A finding one check made is
the specific one; a finding ninety-nine made is a census. Nothing is hidden - the count is exact,
three members are named, and the rest are counted - which is the difference between grouping and
truncating.

Same reasoning as the rule already in `print_divergences`: a check only one side ran is not listed
at all, because listing it buries the ones that are (D622). This is that rule applied one level
in, to entries that *are* comparable and are all saying the same thing.

## What it exposed

With the list readable, seven of the surviving sixteen turned out not to be orbistoun's at all
(D626), and the differential's own headline had been counting them as defects for a day. A number
that is the size of a list is not a number of findings, and a tool that reports the first as the
second is doing the same thing principle 3 forbids a stub for doing.
