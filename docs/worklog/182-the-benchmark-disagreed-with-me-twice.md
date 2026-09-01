# 2026-08-27 - The benchmark disagreed with me twice before it was right


`./orbistoun.sh suggest benchmark` asks every configured entry the same question and
reorders the ladder by the answer, replacing the argument that used to order it (D334).

**Ranking by speed picks the worst engine.** A local 4B model answers in under a second and
an installed coding assistant takes a minute - and the fast one is measurably worse at this.
A round's real cost is the sweep afterwards anyway, beside which the call is a rounding
error.

**Ranking by volume sees nothing.** Both engines return twelve words when asked for twelve,
on two runs, on an easy question and the real one.

**Ranking by novelty is the one that works** - words this machine does not already hold,
which is exactly what the loop values, since a proposal already in the vocabulary is refused
before it costs anything. Claude Code 9 of 12; the local model 2 of 12, because ten of its
twelve were already known.

Two fidelity bugs on the way, both mine: the benchmark asked its own easier question instead
of the caller's, and asked at temperature zero where the loop asks at 0.9. A question easier
than the work measures nothing about the work.

**And I had claimed too much earlier.** I reported one run of each engine as 29 accepted
words against 8 and called it decisive. One run each is not a measurement. The conclusion
held up; the evidence I offered for it did not.

The registry now leads with `claude-code`, written by the benchmark rather than argued for.

### Two shapes were capping the vocabulary by 33x

Asked whether `prefix-module-verb-learned-learned` was a waste. 0 of 323 records is weak
evidence - a shape produces nothing when the *vocabulary* lacks the words - so the corpus
analysis settled it instead. Of 8,417 vendor-shaped names: 1,025 blocked on a missing shape,
**6,966 not splittable into known words at all**. Vocabulary is seven times the constraint.

`learned` twice in a shape makes a round quadratic in it, so at the 2.6-billion budget those
two shapes cap the list at **483 words against 16,042**. Not a waste - self-defeating: ~145
forecast names paid for with a cap that leaves 6,966 names unreachable.

Disabled rather than deleted, with a **mandatory reason** - `disabled` is an `Option<String>`
and presence is the disabling, so nothing can be switched off without recording what it cost
and what brings it back. Validated before filtered, so a disabled shape cannot hide a broken
vocabulary reference until somebody re-enables it.

**The price was exactly what the record count predicted**: two names,
`sceAudioPropagationPortalDestroy` and `sceAudioPropagationSystemDestroy`, both
prefix-module-learned-learned-verb. On the ceiling with the reason; they return with the
shape.

### What to expect next run

At linear cost the harvest that would have grown `learned` to 5,592 words now sits at 906
million against a 2.6-billion budget, so `names` will accept it rather than refuse. That is
the trade being taken. The mangling fragments the filter still misses ride along with it -
worth cleaning at the source rather than tolerating because they are now cheap.

### The harvest cost is reported now, and the filter was refused

**Reported.** `learn_words` refuses loudly when a vocabulary would break the budget (D330)
and accepted *silently*, so a list could go from 177 words to thousands with nothing saying
so - D320 again, at a price nobody is told about. The harvest now prints what it added and
what a round costs before and after, and flags a doubling.

**Refused.** The obvious next filter was letters-then-digits, to catch `A0`, `A021311`,
`Storage14`. Measured against the parts that vendor names actually use, it would strand real
vocabulary:

```
Matching2  Ngs2  Http2  Api2  Utf8  Utf32  Utf16  Ucs2  Iso2022  Big5  Mp4  Int64
```

The narrower rule - five or more consecutive digits, which catches `A021311` - fails too:
`Cp50221`, `Cp51932` and `Gb18030_2000` are real codepage names in the database.

**So the junk is not separable from the vocabulary by shape.** Any spelling rule that catches
`A021311` also catches something real, and a word that goes missing strands names proved by
hash long ago (D259). The cost gate is the right instrument for this and the filter is not:
one bounds what the waste can cost, the other tries to identify it and cannot.

### The way it was nearly missed

The shell line that produced the second measurement printed
`(none above = a 5+ digit run never appears in a vendor name part)` - a label asserting the
conclusion, directly above five counter-examples. Written before the data and left standing
after it. The same failure this log keeps recording, in the command doing the checking.

