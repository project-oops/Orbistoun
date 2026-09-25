# D712 - a submission's draws run at submit, together, into a target they can be written back to

**Status:** assumed
**Date:** 2026-09-24

## The question

D705 lets a fence be written only for work that really ran, and D710 carries out the command
processor's memory work at submit, stopping at the first packet that needs the GPU. Every draw was
such a packet. A GL title's frame loop submits a frame's draws and waits on the fence after them, so
both fully-owned baselines stopped at their first draw submission: the cube after one frame, and
Neverball before its title screen (worklog 830). What has to be true for a draw to count as work
that ran?

## The choice

A submission's draws are **carried out at submit**, on the device, and **what they drew is written
into the guest's colour target** where the guest reads it. Only then does the command processor pass
the draw packets as done, so the `RELEASE_MEM` after them retires from work that ran. Four
constraints keep this exact rather than plausible:

1. **Together, only when that is the same as in turn.** The translator renders a submission as one
   frame. So the draws run as one, at the first draw, and only when every packet between the first
   draw and the last is a draw or memory-inert. Otherwise a fill or copy between them would move, so
   the stream stops *by name* (`Stopped::DrawsInterleaved`) before any draw runs.
2. **Into one target a frame can be written into exactly:**
   - one `CB_COLOR0_BASE`;
   - one resident target of the same extent;
   - `64KB_R_X` tiling, whose whole-surface layout run 18 read back at 1080p (worklog 831);
   - `8_8_8_8` `UNORM` in the order `CB_COLOR0_INFO.COMP_SWAP` names (standard or alternate, from
     Mesa `ac_formats.c:614-619`).

   Anything else leaves the draws unexecuted.
3. **From what the guest's memory holds.** The target starts from its own contents, detiled
   (`REQ-...77fa`). A clear the command processor filled, or the frame before, is what the draws draw
   over.
4. **All or nothing.** A single refused command, a device error, or a frame of the wrong size means
   the frame is not the one the draws make. The executor then answers `None`, nothing is written, and
   the draw is the honest stop it was.

The executor is installed by the worker (`install_draw_executor`), since `orbistoun-gpu` may not
depend on a graphics runtime (principle 12). It runs on a **host thread of its own** while the guest
thread waits: the submit runs on a guest stack that the host's structured exception dispatch cannot
walk, and the first exception the driver raised there (`OutputDebugString`'s `0x40010006`) ended the
process.

## Why not the alternatives

- **Post the fence and render later**: this is what D705 declined. A fence for draws that never ran
  tells the guest its pixels exist. The GL context's own readback would then read an unwritten target.
- **Run each draw at its own packet**: exact in every case, but it needs the translator to render per
  draw with state carried between calls. Nothing in the two baselines needs that yet: their draws are
  contiguous. Constraint 1 names the case when a title does.
- **Keep the frame host-side and skip write-back**: the guest reads its target. The GL context copies
  it to a readback buffer, and a later frame composes over it. A frame that exists only on the host is
  a frame the guest never drew.

## Consequences

- The cube runs five frames, every submission to completion, and the SDK's own frame loop confirms
  them (`frames-confirmed 0x5`). Neverball's first draw submission retires and it submits its next
  frame, where a translated mesh draw loses the device (worklog 832).
- Driving one frame costs seconds: 12 s for Neverball's 450 draws, because each draw reads the
  1080p attachment in and out. Correct first. The cost is a named item, not a reason to post early.
