# D754 - An unwritten index size is sixteen bits

**Status:** decided
**Date:** 2026-10-07

An indexed draw whose stream, and every submission carried into it (D737), never wrote
`VGT_INDEX_TYPE` reads sixteen-bit indices (`INDEX_TYPE` 0, `VGT_INDEX_16`). A size the stream did
write is read as written, and the eight-bit size is still refused. This narrows D740, which refused
an unwritten size. It is `assumed`, so a hardware row showing another reset value retires it.

**Why:** PPSA28061 draws with `DRAW_INDEX_OFFSET_2` after `sceAgcDcbSetIndexBuffer` and
`sceAgcDcbSetIndexCount`, and imports no `sceAgcDcbSetIndexSize`. The measured `SetIndexBuffer`
emits only `INDEX_BASE` (`166-agc/dcb-set-index-buffer`). Nothing in its submissions writes the
register, directly or by an indirect uconfig load. The six indices it binds read `0, 1, 2, 0, 2, 3`
as sixteen-bit values: one quad. As thirty-two-bit values they would be `0x10000, 2, 0x30002, 0, 0,
0`, which no six-index draw names. So the size in force on hardware is sixteen bits, and refusing
the draw only hides that.

**Rejected:**
- Keeping D740's refusal. It holds every draw of a title that relies on the queue's size.
- Inferring the size per draw from the index data. A guess from data is a per-title heuristic, and a
  thirty-two-bit buffer of small indices reads as plausible sixteen-bit ones.
