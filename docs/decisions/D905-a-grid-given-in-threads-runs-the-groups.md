# D905 - A grid given in threads runs the groups covering it

**Status:** decided
**Date:** 2026-10-19

When a dispatch initiator sets `USE_THREAD_DIMENSIONS`, its dimensions count threads, not groups.
It runs, along each dimension, the groups that cover the grid. Where the group size does not divide
the grid, the last group runs the remainder: `PARTIAL_TG_EN`, and `COMPUTE_NUM_THREAD_*`'s partial
count equal to that remainder. This is how radv sets up an unaligned dispatch
(`radv_cmd_buffer.c:14795-14863`), and it reuses the partial last group already modelled.

A remainder with no partial group is refused, and so is a partial count that is not the remainder.
What the hardware does in either case is unmeasured.

**Why:** Grand Theft Auto V's first loading-screen dispatch gives `0x4800` threads in groups of 64
under initiator `0x61`, and was refused.
