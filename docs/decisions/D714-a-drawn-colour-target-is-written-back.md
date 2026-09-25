# D714 - a drawn colour target is written back at the flip, not after every submission

**Status:** decided
**Date:** 2026-09-24

## The question

D712 carried a submission's draws out at submit and wrote the frame back into guest memory before
the fence after them retired. A GL title submits dozens of times per frame, whenever its vertex ring
fills. Neverball submits ~40 times a frame into one 1080p target. So the full 8 MB round trip -
read back from the device, re-tile, write into guest memory - ran for every one of them and capped
the title at about one frame a second. When does the guest actually need the frame in its memory?

## The choice

**When it flips.** The frame stays on the device across a frame's submissions and is written into
guest memory when:

- the guest flips (before anything reads the flipped buffer), or
- a submission draws into a different target (the executor holds one frame per extent).

A submission's draws still run on the device before its fence retires, so the fence still stands
for work that ran. What moves is only when the result is copied back into guest memory.

Unchanged is detected, not assumed. A submission reads its target from guest memory, and if the
bytes are exactly what was last written or read there, the executor keeps what it holds. If
anything else wrote the target in between, it is re-read.

`ORBISTOUN_TARGET_WRITEBACK=submit` restores a write-back after every submission.

## What it costs

- A guest that reads its own colour target with the CPU between submissions and before the flip
  (a `glReadPixels`-style readback) sees the last written-back frame, not the latest draws. Use
  `submit` for such a guest.
- If something other than the draws partially writes a target whose frame is pending (a DMA fill
  over part of it, say), the next submission re-reads guest memory and the pending draws are lost.
  A full clear discards them anyway.

The user chose this (2026-09-24), after being told both costs.

## Amended (worklog 849)

The second cost is gone, along with a third that went unlisted. Any command-processor work that touches
a pending target writes the frame back first: a DMA fill, a DMA copy's source or destination, or an
event write. The GL context copies its colour target into its readback buffer at the end of every
submission, so that copy read the frame from before the submission's draws. `glReadPixels` and
`glGetFrameReadbackSampled` read that buffer. A GL frame now takes two write-backs rather than one.
The first cost, a CPU read of the target itself, still stands.
