# D714 - A drawn colour target is written back at the flip

**Status:** decided
**Date:** 2026-09-24

A frame stays on the device across a frame's submissions and is written into guest memory at the
flip, when a submission draws into a different target, or before command-processor work touches
the pending target. Unchanged is detected by comparing the target's bytes with those last
written or read, and `ORBISTOUN_TARGET_WRITEBACK=submit` writes back after every submission.

**Why:** a GL title submits dozens of times a frame into one target, and an 8 MB readback and
retile per submission capped it near one frame a second. Draws still run before their fence
retires, so only the copy moves. A CPU read of the target between submissions sees the last
written-back frame; the operator chose this knowing it, and `submit` serves such a guest.

**Rejected:**
- Write-back after every submission: exact, and the dominant cost of a frame.
- Assuming the target unchanged between submissions: misses writes by anything else.
