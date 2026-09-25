# D719 - a flipped frame reaches guest memory when something reads it

**Status:** assumed
**Date:** 2026-09-25

## The question

D714 writes the frame drawn since the last flip back into guest memory at the flip. That means
reading 8 MB off the device, then tiling it into the target, on the guest's thread, at every flip:
~7 ms a flip, ~80 ms of every Neverball second. On the console the draws wrote that memory
directly, so the flip costs nothing. Can the write-back stay exact without being paid at the flip?

## The choice

**The flip defers the write-back the way D717 defers a copy.** At the flip the pending target
becomes a deferred copy onto itself:

- the frame is kept in a device snapshot;
- the target's memory as the draws started from it is kept, shared;
- the target's host pages are guarded.

The first thing that touches those pages carries the write-back out, before its access proceeds:

- the guest, through the fault handler;
- the command processor, through `carry_out_overlapping`;
- a draw reading the target, through the same;
- the window.

**A command-processor fill that covers a deferred destination completely drops it unread.** Every
byte the write-back would produce is overwritten before anything can observe it. The GL context
clears each target at the start of its frame, so a buffer nobody read is never written back at all.

**The window takes the flipped frame from the device, off the guest's thread.** What the display
scans out is the target's memory. Over the visible pixels that is the snapshot, tiled with the
target's byte order and detiled with the scanout's, so it is computed directly from the snapshot
by those same two maps. The display is not something the guest can observe.

## Why it is exact

Nothing can read the target's memory between the flip and the carry-out, because every reader
either faults or asks first. The bytes produced are the ones D714 would have written at the flip.
A reader that is not covered is a fault in this decision, found the way D717's were: the gl1-probe
front-buffer tests read the front buffer after a swap.

## The guard spans host regions

Guest direct memory is mapped in views of its own: an 8 MB target lies across five 2 MB views.
The guard D717 used accepted only a range inside one host region, so every flip fell back to an
immediate write-back. It now requires one committed protection across the range and changes it a
region at a time, rolling back if the host refuses one.

## What it rests on

- D714, the pending target.
- D717, deferred copies, snapshots, guards and the fault handler.
