# D984 - In-world frame costs: host page queries, texel offset tables and dispatch buffer size classes

**Status:** decided
**Date:** 2026-10-25
**known_by:** measured

A lazily committed reservation keeps the spans it knows are committed. A commit inside those spans
asks the host nothing, because nothing decommits a page while its reservation is held; the fault
path consults the same record first. A write-watch harvest goes one recorded reservation at a time
and asks the host for the written pages directly, never where its regions end. A guard is set or
released over its whole range in one protection call where the host allows it, and piece by piece
only where it does not.

Detiling and tiling read each layout's offsets once per column and once per row into an offset
table, then place each texel with two lookups, an add and an exclusive or. This rests on a
block-tiled offset being the block's start plus the exclusive or of a term of x, a term of y and a
constant; tests check the tables against the texel-by-texel path for every layout, with and without
the pipe-bank exclusive or. Detiling is spread over threads.

Dispatch buffers are kept by size class, each within a quarter of the size asked for, and nothing
past the size asked for is read (amends D805). A dispatch image's bytes are detiled where they lie
rather than copied first.

**Why:** Grand Theft Auto V's in-world prologue took 40.1 s of command-processor time per flip,
measured from its first in-world window with the command processor sampled. Of each flip:
- releasing guards before write-back took 9.23 s, almost all of it host page queries, each costing
  milliseconds in the title's large allocations;
- detiling eight-byte targets ran at about 262 MB/s, a quarter of the sampled time;
- exact-size buffer reuse made 4270 new dispatch buffers.
After these changes a flip takes 13.8 s (0.025 to 0.072 flips a second). Releasing guards takes
0.3 ms, dispatch preparation 9.48 s to 3.73 s, dispatch write-back 11.5 s to 1.55 s, and new buffers
fell to 944. Other changes moved between the two runs, so the per-span figures, not the totals, tie
each gain to its cause.

**Rejected:**
- Asking the host about every range before each write-back: correct, but its cost grows with the
  whole allocation, not the range asked about.
- Marking a whole reservation in one harvest: it pays for reporting every page written anywhere in
  it.
- Reusing a buffer only at its exact size: titles vary sizes by a few words per dispatch, so nearly
  every dispatch made a new one.
