# D761 - A chunk of a draw counts its vertex ids from a draw word

**Status:** decided
**Date:** 2026-10-08

A chunk of a draw with no indices (D741) is drawn as a plain draw of its vertices from its first
vertex. Its primitive shader reads that first vertex from a word of its draw data,
`DRAW_DATA_FIRST_VERTEX_WORD`, and counts its vertex threads' ids up from it. It no longer reads
them from a bound buffer of counted ids. Every chunk of the draw therefore runs one module over the
same buffers, and only its draw words differ. A chunk of an indexed draw still binds its own indices.

**Why:** CRFT00001 draws its world as large non-indexed draws, about 270 chunks each. Every chunk
bound its own buffer of counted ids, so every chunk re-bound the draw's buffers and placed them in
the arenas again. That was 8,828 binds against 33 user-data changes in one submission. With the
first vertex in the words, the chunks join a batch as one draw's do, and CRFT00001 goes from 5 to 7
frames a second.

**Rejected:**
- A module per chunk with its first vertex compiled in: hundreds of modules for one draw.
- Caching each run of counted ids: the buffers still differed per chunk, so every chunk still
  re-bound them.
