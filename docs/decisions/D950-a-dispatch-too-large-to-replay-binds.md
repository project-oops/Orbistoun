# D950 - A dispatch too large to replay binds its global loads from a sample of its groups

**Status:** decided
**Date:** 2026-10-24

A dispatch whose global loads D883 would run through more waves than one preparation runs is no
longer refused. Every wave of a sample of its groups runs instead. Along each dimension the sample
takes the first and last group and as many evenly spaced between as the bound on waves allows, and
it runs every combination of them. The corners of the grid are always in the sample. A load whose
address is an affine function of the group and thread ids, plus values fixed for the dispatch, reads
its lowest and highest bytes at a corner, so the sample finds its range exactly.

The range bound is what the sampled waves read, widened by a margin on each side: an eighth of its
bytes, at most a megabyte, in whole pages. The margin is kept only where guest memory is mapped
across it. It gives way where it would overlap another buffer the dispatch binds and what the waves
read would not.

The sample places the range; it does not vouch for it. A traced global load in a dispatch's module
checks every word it reads against the bound buffer's length. A word below the base or past the end
is an escape, as a buffer access outside the window is. The dispatch runs, and its result is refused
after it, as D746 refuses any escape. A sub-word load's next word counts only where the load's bytes
reach into it. A load is never answered from a word the buffer does not hold.

The run report notes once per program that a dispatch's global loads were bound from a sample: how
many groups ran, what they read, and the range bound.

**Why:** Grand Theft Auto V's in-world frame runs a compute pass of 240 x 135 groups, a 1920x1080
image in 8x8 groups. That is 32,400 groups, eight times the waves one preparation runs. Its global
loads were refused on every frame, its writes never landed, and the world could not appear.

**Rejected:**
- Raising the bound on waves. One wave run on the host for each of a 4K pass's groups costs far
  more than a frame, and the bound exists so a dispatch cannot ask that.
- Proving each load's address affine and deriving its range from the corners alone. It needs a
  symbolic run beside the concrete one that D753 keeps the single semantics. The corners in the
  sample give the same range for an affine address. The escape check covers every other case,
  where a proof would have refused the load.
- Binding the sampled range without the check. A table indirection, or a branch only the unsampled
  groups take, reads outside it, and the dispatch would then answer from word zero.
