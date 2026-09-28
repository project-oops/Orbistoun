# D729 - A stream's draws run in segments between its memory work

**Status:** decided
**Date:** 2026-09-27

Memory work between two draws splits a stream's draws into segments, amending D712. The command
processor carries each segment out in order: the memory work before it on the CPU, then its draws
together on the device, prepared from the stream up to its last draw with every earlier draw
replaced by a `NOP`, so every register write still reaches them. A fence between segments
retires only after the draws before it ran.

**Why:** radeonsi separates the draws into different targets of one stream with end-of-pipe
fences and prefetches (Craft's texture clears), which D712 left unexecuted. Silencing the other
segments' draws keeps each segment's register state exactly the stream's, with no state carried
by hand.

**Rejected:**
- Each draw at its own packet: the draws of a segment share a target and run as one frame.
- Reordering the memory work around the draws: moves a fence ahead of the draws it stands for.
- Replaying register state into a fresh stream per segment: a second model of register state
  beside the one the pipeline already decodes.
