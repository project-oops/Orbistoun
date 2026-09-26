# D558 - Flipped is a reach rung, and frames rank below imports

**Status:** decided
**Date:** 2026-09-04

The reach ladder has one rung above `Entered`: `Flipped`, reached when a video-out port accepts a
flip, counted from the port table by `orbistoun_video::flips_accepted`. The ranking orders reach,
imports, answered, standing, frames, calls.

**Why:** an accepted flip requires opening an output, setting buffer attributes, registering
buffers and configuring the output against real implementations, so no guest can spin into it.
It claims only that the guest reached the presentation layer, not that a picture was shown.
Frames sit below imports because a guest can present one buffer forever, and above calls because
a frame is achieved where a call is only counted.

**Rejected:**
- A survival rung: surviving to the time limit ranks the least informative run highest.
- Counting submit calls from the trace: credits a flip the port refused.
- Frames above imports: a present loop outranks a run that got further into the engine.
