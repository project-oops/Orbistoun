# 419. Six arguments, and a hypothesis that was wrong

**2026-09-04** - directed

## What was done

`RecordedCall` and `TracedCall` now carry all six integer arguments instead of the first, and
`error_used_as_pointer` scans every one - naming the register, because saying *"its first
argument"* when the value sat in `rdx` sends a reader to the wrong place.

Cheap to try, because recording already stops after `MAX_RECORDED_CALLS`: the extra stores are
bounded at 8,192 × 5 however long a guest runs.

## And it did not work

The reasoning was that a placeholder handed on as a *size* is invisible unless the size is the
first argument - D564's four gigabytes were only ever seen because `malloc`'s size is `arg0`.
Sound, and **wrong**.

Six titles re-run under tagging, arguments verified reaching the trace, and **zero placeholders
appear as arguments to anything.** The attributable count went from one to one.

So the constraint was never `arg0`. Within the recorded window the guest does not pass these
answers on at all - it stores them, tests them, or drops them. The one attributable case remains a
value still sitting in `rbx` at a fault.

**Kept anyway.** A report that looks at one of six arguments and calls it *"its first argument"* is
describing a fraction of what it was handed; scanning all six is more correct whether or not this
corpus rewards it. And ruling out the cheap explanation is what makes it reasonable to consider the
expensive one - the 48-call window (D568) is now the only suspect left.

## The finding that did come out of it

**Guest registers hold values that look exactly like tags.** PPSA28061's tail carries `0x7fff0201`
and `0x7fffbe01` in argument slots. Both decode cleanly to stub indices - 497 and 48,625 - and
**neither was ever called** in that run. Stale register contents landing inside the reserved range.

`source_of` requires a decoded slot to match an import the run actually called, and that filters
both. It was written as ordinary care; it turns out to be the only thing between tagging and
**confident wrong attributions assembled from garbage**, which would be worse than the vague
finding tagging replaced. Now guarded with those two real values instead of an invented one.

## Two gates, again

**`multiple_unsafe_ops_per_block` is deny** (principle 4), and `args.add(register).read()` is two
operations. Split, with an invariant stated for each half rather than one comment covering both.

The `015-sync` timed-wait test failed in the full workspace run and passes alone - the documented
load-sensitive case. Re-run, not weakened.

## What is not claimed

**That widening the window would work.** It is the remaining suspect, not a demonstrated cause.

**Nor that the 97 are classifiable at all.** A function whose answer the guest never uses as data
leaves no evidence anywhere, and for many of these that may just be the truth: called once, answer
ignored, nothing knowable from a run.
