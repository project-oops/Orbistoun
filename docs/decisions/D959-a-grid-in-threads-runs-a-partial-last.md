# D959 - A grid in threads runs a partial last group

**Status:** decided
**Date:** 2026-10-24
**known_by:** guest-observed

A dispatch that gives its grid in threads (`COMPUTE_DISPATCH_INITIATOR.USE_THREAD_DIMENSIONS`) runs
exactly those threads, whether or not `PARTIAL_TG_EN` is set: where the group size does not divide
the grid, the last group runs the remainder alone (D905). This supersedes D940, which ran the whole
last group when the bit was clear.

**Why:** Grand Theft Auto V's hierarchy refit (`image+0x3ce6d00`) is dispatched as 296 threads in
groups of 64 under initiator `0x61`, with no partial group, and has no bound of its own. Run whole,
the 24 threads past the grid read a missing parent link and arrived at the root as 24 extra
children; the treelet optimiser after it (`image+0x3dc9a00`) found the root the one node neither
arrival stopped at, restructured it from two waves at once, and left a tree with back edges that a
later walk (`image+0x3cf7000`) never finished, until the device was lost. With the root's count as
a refit stopping at the grid leaves it, the optimiser's ten concurrent groups gave one valid tree in
every replay. The program is consistent only if threads past the grid do not run; a hardware census
of which threads run under `0x61` would make this `measured`.

**Rejected:**
- Keeping whole groups and relying on programs to bound themselves: this one does not, and the
  console evidently does not need it to.
