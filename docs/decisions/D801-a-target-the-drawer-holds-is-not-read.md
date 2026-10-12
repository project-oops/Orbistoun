# D801 - a target the drawer holds is not read again

**Status:** decided
**Date:** 2026-10-12

The drawer keeps each colour target it draws. Before a submission draws over a target, orbistoun
asks whether the drawer's copy is still what memory holds. Until now it remembered one target, the
last it wrote or read, so a frame alternating between two display buffers re-read and detiled the
whole of one of them on every submission.

Two more cases now answer "held":

- **An earlier target whose bytes are unchanged.** The last few targets before the last keep the
  bytes their memory held when they were last written or read. One answers held when memory holds
  exactly those bytes again. Nothing but the bytes vouches for it: the write protection and the
  write-watch mark follow the last target only.
- **A target under its own deferred frame.** A target can have the frame drawn into it still
  deferred onto it from its flip (D719), taken over exactly the bytes recorded for it, with no other
  deferred write over it. Nothing has touched it since, because the copy's guard would have carried
  the copy out, and memory will hold the frame the drawer already has. It answers held without the
  copy being carried out. Once the new draws leave their frame pending, the deferred copy is
  superseded: the new frame writes every byte it would. It is then dropped unread, as a later flip's
  write-back drops an earlier one's.

**Why:** PPSA03416 spent about 700 ms of every second reading its two 3840x2160 display buffers
back: a deferred write-back carried out by the check, then a full read and detile. With both cases
it spends about 10 ms, and runs 1353 submissions in 120 s on the host clock where it ran 384.

**Rejected:**
- Write protection or write-watch for every kept target: one protected target is the model D720
  builds on, and a byte compare of a few kept targets costs a few milliseconds.
- Dropping the deferred copy as soon as the target answers held: a drawer that then failed would
  leave memory without the frame. The copy is dropped only after the new frame is pending.
