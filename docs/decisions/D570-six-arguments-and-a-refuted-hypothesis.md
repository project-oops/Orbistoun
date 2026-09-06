# D570 - Six arguments, a refuted hypothesis, and a filter that turned out to be load-bearing

**Status:** measured
**Date:** 2026-09-04

## The hypothesis

An attempt to classify the 97 called-but-unimplemented functions with no `returns` - by tagging
placeholders and seeing what the guest did with each - found **one** usable observation across four
runs. The proposed cause was that `TracedCall` recorded only `arg0`, so a placeholder handed on as
a *size* was invisible unless the size happened to be the first argument. D564's four gigabytes
were visible only because `malloc`'s size is `arg0`.

That reasoning was sound and **it was wrong.**

## What was built, and what it showed

`RecordedCall` and `TracedCall` now carry all six integer arguments, and
`error_used_as_pointer` scans every one of them rather than the first - naming the register, since
saying *"its first argument"* when the value was in `rdx` sends a reader to the wrong place.

It cost nothing on the hot path, which is why it was cheap to try: recording already stops after
`MAX_RECORDED_CALLS`, so the extra stores are bounded at 8,192 × 5 however long a guest runs.

Six titles re-run with tagging. The arguments are demonstrably reaching the trace. And:

**Zero placeholders appear as arguments to anything.** The attributable count went from one to one.

So the constraint was never `arg0`. Within the recorded window the guest simply does not pass these
answers on - it stores them, tests them, or discards them, and the one attributable case is a value
still sitting in `rbx` at a fault.

## Kept anyway, and why that is not sunk cost

Scanning every argument is **more correct** whether or not this corpus rewards it: a report that
looks at one of six arguments and says "its first argument" is describing a fraction of what it
was handed. The change stands on that, not on the classification it failed to enable.

The remaining suspect is the observation window - 48 calls out of hundreds of thousands (D568) -
and this rules out the cheaper explanation before anyone spends effort on the expensive one.

## The finding that actually came out of it

**Guest registers hold values that look exactly like tags.** PPSA28061's tail carries `0x7fff0201`
and `0x7fffbe01` in argument slots. Both decode cleanly to stub indices - 497 and 48,625 - and
**neither was ever called** in that run. They are stale register contents that happen to land in
the reserved range.

`source_of` requires a decoded slot to match an import the run actually called, which filters both.
That check was written as ordinary care and turns out to be the thing standing between tagging and
**confident wrong attributions built from garbage** - strictly worse than the vague finding tagging
replaced. It is now guarded with those two real values rather than a made-up one.

## What this does not establish

**That widening the window would work.** It is the remaining suspect, not a demonstrated cause.
Making the ring circular would test it and would trade away a boot's opening, which D568 left open
deliberately.

**Nor that the 97 are classifiable at all.** A function whose answer the guest never uses as data
leaves no evidence anywhere, and for many of these that may simply be the truth: they are called
once, their answer is ignored, and nothing about them is knowable from a run.
