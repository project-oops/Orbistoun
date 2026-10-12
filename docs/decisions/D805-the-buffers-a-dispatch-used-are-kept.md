# D805 - the buffers a dispatch used are kept for the next

**Status:** decided
**Date:** 2026-10-12

A compute dispatch binds its own buffers (D746) through a pair per slot: a host staging buffer
its words are copied through, and a device-local buffer the program reads and writes. When the
dispatch finishes, these pairs are kept rather than destroyed. The next dispatch that needs a
buffer of the same size, use and placement takes one of them. The kept buffers hold at most
160 MiB together; past that, a released buffer is destroyed as before.

A reused buffer is never read for what it held. A staging buffer is written whole with the new
dispatch's words before it is copied anywhere. A bound buffer is written whole by the copy from
its staging buffer before the program runs.

**Why:** every dispatch made and destroyed a pair for each of the draw-buffer slots, placeholders
included: 2244 allocations in 40 s of PPSA03416, and the device thread spent about a third of its
time in the driver's allocation calls. With the pairs kept, PPSA03416 runs 2738 submissions in
120 s on the host clock where it ran 1566, and PPSA28061 4986 where it ran 4396.

**Rejected:**
- Keeping one set of buffers per slot across dispatches, resized on demand: a dispatch's buffers
  differ in size from call to call, and resizing destroys and allocates just the same.
