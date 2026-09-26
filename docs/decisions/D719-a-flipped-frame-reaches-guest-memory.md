# D719 - A flipped frame reaches guest memory when something reads it

**Status:** assumed
**Date:** 2026-09-26

At a flip the pending target (D714) becomes a deferred copy onto itself (D717): the frame is kept
in a device snapshot, the target's pages are guarded, and the first reader - the guest through the
fault handler, the command processor, a draw, or the window - carries the write-back out first. A
command-processor fill that covers a deferred destination completely drops it unread, and the
window scans out from the device snapshot, off the guest's thread.

**Why:** writing 8 MB back and tiling it on the guest's thread at every flip costs about 7 ms a
flip, where on the console the draws wrote that memory directly. Nothing can read the target
between the flip and the carry-out, because every reader either faults or asks first, so the bytes
produced are the ones D714 writes at the flip. A fill that covers the target overwrites every byte
before anything can observe it. The guard spans host regions: guest direct memory is mapped in
views of its own, so protection is changed a region at a time and rolled back if the host refuses
one.

**Rejected:** write-back at every flip - exact, but paid on the guest's thread whether or not
anything reads the frame.
**Rejected:** a guard confined to one host region - an 8 MB target spans five 2 MB views, so every
flip falls back to an immediate write-back.
