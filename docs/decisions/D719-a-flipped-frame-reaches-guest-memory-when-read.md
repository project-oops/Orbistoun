# D719 - A flipped frame reaches guest memory when read

**Status:** assumed
**Date:** 2026-09-26

At a flip the pending target becomes a deferred copy onto itself (D717): the frame is kept in a
device snapshot, the target's pages are guarded, and the first reader, whether the guest through
the fault handler, the command processor, a draw or the window, carries the write-back out
first. A command-processor fill or copy that covers a deferred destination completely drops it
unread, and the window scans out from the device snapshot, off the guest's thread.

A command-processor fill of whole host pages is deferred the same way, and carried out as a fill
by the first touch. A draw is handed a target under a deferred fill as uniform without its
memory being read. A copy out of one with nothing drawn over it is a fill of the copy's
destination. A deferred copy of a frame never reads its source's memory, so it leaves a fill
under that source deferred.

**Why:** writing 8 MB back and tiling it on the guest's thread at every flip costs about 7 ms a
flip, where on the hardware the draws wrote that memory directly. Every reader either faults or
asks first, so the bytes produced are the ones an immediate write-back would give, and a
covering fill overwrites every byte before anything can observe it. A frame's clears are two
8 MB fills a flip, written by the command processor on the guest's thread and read back by the
first draw, where on the hardware they are the GPU's. Guest direct memory is
mapped in views of its own, so protection changes a region at a time and rolls back if the host
refuses one.

**Rejected:**
- Write-back at every flip: exact, but paid on the guest's thread whether or not anything reads
  the frame.
- A guard confined to one host region: an 8 MB target spans five 2 MB views, so every flip falls
  back to an immediate write-back.
