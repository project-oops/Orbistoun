# D894 - A dispatch that fetches a depth target reads the host's depths

**Status:** decided
**Date:** 2026-10-19

The host holds a depth target as its own depth attachment and never writes it to guest memory
(D817). A compute dispatch can reach such a target as a single-level two-dimensional `32_FLOAT`
image in one of the depth swizzles (`ADDR_SW_*_Z*`, `addrtypes.h`). It names that target by the
descriptor's address, as a depth texture does, and does not read guest memory for it:

- A fetched image of a depth target is filled, on the device, with the attachment's depths as the
  draws before it left them, one float a texel. These are the depths a depth texture reads.
- A stored image of a depth target starts from those depths. What the dispatch leaves in it then
  replaces the attachment's depths, and it is not written to guest memory.

A target no draw on the host drew is refused by name, as is one whose extent is not the image's.

**Why:** Grand Theft Auto V's scene compute program reads its depth buffer, `32_FLOAT` in
`64KB_Z_X` at the address of the depth target its draws bound, and writes it back. On the console
the draws and the dispatch share that memory. Here the draws' depths exist only in the host's
attachment, so reading guest memory would give the dispatch depths no draw wrote. A device test
reads a drawn target's depths back as the draw left them, and one writes them and reads the new
ones back.

**Rejected:**
- Detiling `64KB_Z_X` from guest memory: the addrlib equation is read
  (`tools/addrlib/equation 64KB_Z_X 4`), but guest memory never holds what the draws wrote, so
  the dispatch would read stale depths exactly.
- Writing every depth target back to guest memory after each submission: a 3840x2160 target is
  32 MiB re-tiled each time, for the one pass that reads it.
