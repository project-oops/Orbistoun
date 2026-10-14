# D864 - A guest's write into a frame still on the device costs a band of rows

**Status:** measured
**Date:** 2026-10-14

**Amends:** D720 - a write into the pending frame wrote the whole frame back first. D719 - a touch
of a target under a deferred fill carried the fill out, and then the frame.

A guest's write into its pending frame - drawn on the device, not yet in memory (D714) - is handed
a band of the frame's rows rather than the whole frame: the band is read off the device and laid
into memory, only its pages are made writable, and the rest stays read-only (D720) or guarded under
its deferred fill (D719), which is cut around the band and kept deferred. Before anything takes the
frame from the device - more draws on it, the flip, a copy of it - the band, as memory then holds
it, is patched back onto the device. A frame still pending once its band is folded in is the
drawer's as it stands, and the next submission on it reads nothing. At the flip, the fill's pieces
inside the target are dropped unwritten: the frame's write-back writes every byte of them over the
fill's own word.

A band is whole rows starting on a page: a linear target's rows where a row and a page meet, 32 or
more; a `64KB_R_X` target's one row of blocks, 128 rows, which its layout tiles alone. Any other
target, a band over 2 MiB, or a second write outside an open band, writes the frame back whole as
before.

**Why:** the SDL ports in oops-apps draw a software pointer into the frame before every flip (the
console has no cursor plane). Each did a full write-back - 3.7 ms - after carrying out the frame's
deferred clear - 2.4 ms - and the next submission read the target back in full. Neverball, 30 s:
`write target` 183 -> 5, `carry out` 179 -> 1, for 172 band reads of 0.76 ms and folds of 0.39 ms.
A kept frame, read from the device, shows the pointer the guest drew into memory.

**Rejected:**
- Comparing memory against what it held before the draws to find what the guest wrote: a pointer
  drawn where the last frame drew it writes the bytes already there, and would be lost.
- Asking the port not to draw the pointer: the console does this at no cost, so it is orbistoun's to
  make cheap, not the title's to avoid.
