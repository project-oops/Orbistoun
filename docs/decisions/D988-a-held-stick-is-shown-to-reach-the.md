# D988 - A held stick is shown to reach the picture by frame motion between kept flips

**Status:** decided
**Date:** 2026-10-25

Whether a title answers a stick in its world is measured from the run's own output. The run keeps
a sequence of frames from its first heavy frame (`ORBISTOUN_FRAME_AT=busy:N,busy-every:K`) and
captures what the guest read from its pad (`run --capture-input`). `orbistoun-cli motion` then
takes each pair of consecutive kept frames, measures the mean colour change over a region, and
splits the pairs by whether a stick was off centre at any flip between them. The pairs with a
stick held changing more than the pairs at rest, by a stated factor (`--expect`), is the check.

**Why:** a moving camera changes the whole picture, and a still one leaves it nearly alone, so the
comparison needs no knowledge of the title's memory layout. Pairs at rest in the same phase are
the control: animation, cutscene cameras and noise move the picture too, and only the difference
between the two kinds of pair is the stick's. A script that alternates held and released stretches
gives both kinds in one run.

**Rejected:**
- A per-title player-position address: found by memory scans per title and build, and it says
  nothing about whether the picture follows.
- Comparing one frame before and one after a press: a single pair cannot tell the stick from a
  cutscene camera that happens to move.
