# D717 - a copy out of a colour target on the device is carried out when its destination is first touched

**Status:** decided
**Date:** 2026-09-24

## The question

On the console the GPU and CPU share one memory. When a command stream copies its colour target into
another buffer, the copy reads what the draws just wrote, at memory speed. In orbistoun the drawn
frame lives on the host's device until it is written back (D714). The open-toolchain GL context ends
every submission with an 8 MB copy of its colour target into a CPU readback buffer, and Neverball
submits about 40 times a frame. Writing the frame back before each copy is exact, and it held the
title at 2-3 frames a second. How does a copy stay exact without that?

## The choice

**The copy is carried out when anything first touches its destination.** When the command processor
reaches a copy whose source is exactly the whole pending target:

- The frame is kept on the device as it stands, copied into a snapshot buffer after the draws so far,
  in queue order and unwaited.
- The target's memory as the copy sees it is kept too. It is the bytes the unchanged check already
  holds, shared rather than copied.
- The destination's host pages are made inaccessible.

The first read or write of those pages faults, by the guest or by host code on its behalf. The
worker's fault handler then carries the copy out: it makes the pages accessible, tiles the snapshot
over the kept memory, writes the result, and retries the access. The bytes are the ones the console's
copy would have left there, at the first moment anything could observe them.

A later copy to exactly the same destination overwrites it completely, so the earlier one is dropped
unread. Command-processor work that reads or writes a deferred destination carries it out first, in
place, without a fault.

A copy that is not the whole pending target, or when the destination cannot be guarded (it shares a
host region of mixed protection, or the host is not Windows), is carried out eagerly, as before.

The user chose this (2026-09-24): "I'd rather be slow and accurate than fast and flaky".

## Why this and not the alternatives

- **Write back before every copy:** exact, but it costs a device read-back and a retile per
  submission. The console pays nothing comparable.
- **Defer without guarding:** a guest reading the readback buffer would see stale bytes. That was the
  D714 hole this replaces.
- **Change the SDK to copy only on demand:** that saves console bandwidth too, but orbistoun must be
  exact for any guest, not only ours.

## Consequences

- Correctness is measured: gl1-probe with lazy copies against eager copies gives identical verdicts
  on every shared check, and identical sampled pixels.
- A host thread that faults on a guarded page is serviced the same way as a guest thread.
- `ranges_overlap` against page spans means a guarded page shared with unrelated data carries the copy
  out on the first touch of that data too. That is early, never wrong.
