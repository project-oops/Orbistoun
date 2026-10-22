# D940 - A grid in threads with no partial group runs whole groups

**Status:** decided
**Date:** 2026-10-21
**known_by:** assumed

A dispatch may give its grid in threads (`COMPUTE_DISPATCH_INITIATOR.USE_THREAD_DIMENSIONS`) that
its group size does not divide, without `PARTIAL_TG_EN`. It runs the groups that cover the grid,
as D905 counts them, and the last is a whole group: its threads past the grid run too, and a
program bounds them itself if it must. With `PARTIAL_TG_EN` the last group runs the remainder
alone, as before (D905).

This is the documented meaning of the bit, partial groups enabled or the last group full, not a
measurement; it is recorded `assumed` until a hardware census confirms which threads run.

**Why:** Grand Theft Auto V dispatches 0x477 threads in groups of 64 under initiator `0x61`, with no
partial group. D905 refused it, and every later submission waited behind it.
