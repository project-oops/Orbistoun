# D720 - a colour target is write-protected while it is trusted unchanged

**Status:** assumed
**Date:** 2026-09-25

## The question

Before each submission's draws, the command processor asks whether the colour target's memory has
changed since it last wrote or read it (worklog 844). If it has not, the device already holds it.
The host answers from write-watch (worklog 851), but only for memory the host allocated privately.
Guest direct memory is mapped views, which the host cannot write-watch, so every submission compared
the target's 8 MB against the bytes kept: ~80 ms of every Neverball second. How is "unchanged"
answered there without comparing?

## The choice

**The target's pages are made read-only while it is trusted unchanged.** When the command processor
has read or written a target, its pages become read-only.

- Any write to them faults, whoever makes it: the guest, or host code on its behalf. The fault
  handler records the target as written, gives the pages their protection back and retries the
  write.
- The command processor's own writes release the protection first, rather than faulting.
- So does anything that guards the pages another way (D717, D719).

A target still read-only and unwritten is unchanged. A target written, or never protected, is
compared as before. A write of the very bytes that were there still reads as unchanged by the
comparison.

One target is protected at a time: the one the kept bytes belong to.

## Why it is exact

Page protection sees every write to the pages, as write-watch does. It is conservative in the same
direction: a write of the same bytes is caught as a write and then passes the comparison.
