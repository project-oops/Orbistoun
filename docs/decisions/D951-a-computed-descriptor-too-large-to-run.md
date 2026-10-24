# D951 - A computed descriptor too large to run wave by wave binds from a sample of its groups

**Status:** decided
**Date:** 2026-10-24

A dispatch's computed buffer access that the group-id replay (D914) cannot answer is found by
running its waves on the host (D900). Above 256 waves, the host no longer runs every wave. It runs
every wave of a lattice of groups instead, chosen as D950 chooses them: the first and last group
along each dimension, as many evenly spaced between as 256 waves allow, and every combination. The
corners of the grid are always in the lattice. A group's waves all run, because a waterfall's
descriptor follows its lanes, and another wave of the same group can reach another descriptor.

A lattice that reaches the access with nothing that binds a byte places nothing. That means no
descriptor, or only null ones. An access only some threads take can lie wholly between the sampled
groups, as a waterfall over a tree's leaves of one kind does. The host then runs every wave, as
without a sample, where the dispatch has at most 4096 waves, the bound D883 sets. Above that the
access is refused, saying the sample reached it in no wave. An empty range is never bound from a
sample, because every access through it would escape.

The host replay is bounded in steps as well as in waves. One wave's run stops at its own bound, at
most 2^20 steps. All the waves one preparation runs share 2^22 steps, about two seconds of the host
interpreter. The lattice and any full run share that same budget, and a dispatch's global loads
(D883, D950) get the same budget across their waves. A replay that runs past either bound before
every wave is done refuses the access. The refusal names the bound and the access's byte, and says
how many waves ran. A refused dispatch costs the frame its result, while an unbounded replay costs
the whole run.

What the lattice reaches is bound as one buffer, however many descriptors it holds. The range runs
from the lowest descriptor's base to the highest descriptor's end, each through its offset and
extent as D900 binds one. It is widened by D950's margin: an eighth of its bytes each side, at most
a megabyte, in whole pages. The margin is kept only where guest memory is mapped, and it gives way
where it would overlap a buffer the dispatch already binds. A range larger than one draw buffer
binds is refused. A range that crosses a four-gigabyte span is refused too, because the module
orders addresses by their low halves. A scalar load's base pair is not sampled. It is still refused
unless every wave reaches it with one pair.

The bound buffer starts at its own guest base, not at a descriptor's. The dispatch's push block
carries that base for each buffer placed this way, after the global loads' base, at most eight
buffers. An access through such a buffer is addressed from the descriptor it computes, less the
buffer's base. The module checks every access the descriptor's own bounds admit. The access is an
escape when its first or last byte falls outside the bound buffer's words, when its address carried
past the span's low half, or when the descriptor sits in another four-gigabyte span. A descriptor
whose addressing is not modelled is also an escape. The dispatch runs, and its result is refused
after it, as D746 refuses any escape. An access the descriptor's bounds refuse reads zero on the
hardware too, so it is not an escape. Robust buffer access never stands in for a word the sample
missed.

The run report notes once per program that a computed buffer was bound from a sample: how many
groups ran of how many, what they reached, and the range bound.

**Why:** Grand Theft Auto V's in-world frame spent more than ten minutes preparing dispatches of the
program at `0x400003e1f500` and never got past it. Its computed descriptor depends on its lanes, so
the D914 replay cannot answer it, and D900 ran every wave on the host interpreter, refusing only
above 4096 waves. A dispatch just under that limit ran thousands of full program runs, each with
its own loops, on every frame. The first run with the lattice bound an empty range for the
programs at `0x400003ce6000` (452 groups) and `0x74005b87e400` (632 groups). Their tree walks enter
the waterfall only for one kind of entry, and no sampled group's thread held one.

**Rejected:**
- Lowering the refusal limit. A dispatch above it is refused outright, and the program's writes
  never land.
- Binding the sampled descriptor at its own base when every sampled wave reaches one descriptor.
  An unsampled group that reaches another descriptor would read the first descriptor's buffer at
  its own offsets, with no way to tell.
- Running only the first wave of each sampled group. A waterfall's later waves hold other lanes,
  and the corners argument holds only when every wave of a corner group runs.
