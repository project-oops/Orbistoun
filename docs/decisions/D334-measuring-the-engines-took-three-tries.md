# D334 - Measuring the engines took three tries, and the first two ranked the wrong one


**decided** · 2026-08-27 · asked for, and then the measurement had to be argued with twice

The ladder used to be ordered by an argument: local engines first, because a trace, a fault
address and a guest's own strings are this project's material and the default should not be
to post them to somebody else. **That is dropped**, on instruction, and replaced by
`Llm::benchmark` - which asks every configured entry the same question and reorders by the
answer. Two things had already undermined the argument: the only consumer sends no guest
material - its prompt is library names, confirmed vendor names and English words - and the
measurement put the argument's preferred order the wrong way round.

**Getting the measurement right took three attempts, and the first two are the interesting
part.**

*Ranking by speed* is the obvious design and it picks the worst engine. A local
four-billion-parameter model answers in under a second; an installed coding assistant takes
a minute on the same question. Speed would promote the fast one, and the fast one is
measurably worse at this. It is also the wrong axis twice over, because a round's real cost
is the sweep that follows - billions of candidates hashed - beside which the model's time is
a rounding error.

*Ranking by volume* does not discriminate at all. Asked for twelve words, **both engines
returned twelve**, on two separate runs, on both an easy question and the real one. On that
evidence they are equal, and in the loop they are not.

*Ranking by novelty* is the one that works, because it is what the loop actually values: a
proposal already in the vocabulary is refused before it costs anything. Scoring words this
machine does not already hold:

| | novel words | time |
|---|---|---|
| Claude Code | **9 of 12** | 66.3 s |
| local 4B on an accelerator | **2 of 12** | 0.6 s |

The local model returns twelve words and ten of them are already known. That is the whole
difference, and neither of the first two measurements could see it.

**Two fidelity bugs found on the way, both mine.** The benchmark first asked its own short
question rather than the caller's - a question easier than the work measures nothing about
the work - so `measure` now takes the request and the tool passes the real prompt. And it
asked at the default temperature of zero while the loop asks at 0.9, which is not the same
question either.

**And a claim of mine was wrong.** I reported a single run of each engine as showing 29
accepted words against 8 and called it decisive. One run each is not a measurement, and the
volume figures inside it were equal. The conclusion survives; the evidence I gave for it did
not, and it took a benchmark that disagreed twice to find that out.


