# 849. A DMA that reads a pending target writes it back first

**2026-09-24**. Correctness, then the cost of it.

- **The hole in D714:** the GL context ends every submission with a DMA copy of its colour target into
  a CPU readback buffer, which `glReadPixels` reads. With write-back deferred to the flip, that copy
  read guest memory from before the submission's draws. `GuestCp` now writes a pending frame back
  before any fill, copy (either side) or write touches its surface (`overlaps_target`, tested at the
  boundaries). D714 is amended.
- **Cheaper write-back:** `CpMemory::edit_words` lets guest memory be tiled in place. `write_target`
  was five 8 MB passes: read out, to words, tile, to bytes, write in. It is now a tile plus one
  snapshot for the unchanged check, 6.5 ms down to 2.3 ms.
- Neverball holds 8-10 flips/s with two write-backs a frame.

**The D716 translator changes are exact:** gl1-probe, run with them disabled, reached 13 checks in
60 s against 41 with them. On those 13, every verdict and all 73 sampled pixels are bit-identical.

**gl1-probe fails 34 of 45 checks under orbistoun:** scissor, stencil, clip-plane, logic-op,
depth-range, mipmaps, readbacks and more. These failures predate today's work. They are the
correctness worklist for GL titles.
