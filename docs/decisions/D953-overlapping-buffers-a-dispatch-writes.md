# D953 - Overlapping buffers a dispatch writes bind one buffer

**Status:** decided
**Date:** 2026-10-24
**known_by:** published

Two ranges a dispatch binds that share a byte, where the dispatch writes through either one, are
bound as one buffer. Read-only copies of the same bytes still stay apart, because they agree
(D746). Overlaps join ranges into sets through any member, over each overlap where one side is
written. Each set is held by the range at its lowest base, or the earliest slot on a tie. That
range is widened to end where the set's last range ends. Every other slot in the set binds the
holder's buffer from where its own range starts in it, as the descriptor's buffer offset. Its
binding runs to the holder's end, because the bytes past its own range are the holder's on the
guest too. A slot already bound to another's exact range (D937) follows that slot into the set.

The holder is uploaded once, from the widened range. It is read where any slot in the set reads
it, and written back once, with only the words the dispatch changed or stored. Each write mask is
bound at the same offset as its slot, one word for each word. Vulkan requires a storage buffer
binding's offset to be a multiple of `minStorageBufferOffsetAlignment`, at most 256 bytes. An
offset that is not a multiple, or that falls outside the holder, is refused by name.

Each slot's binding starts at its own range's first byte, the base its accesses are read against:
its descriptor's, or for a buffer placed from a sample (D951) the base the push block carries. Such
a buffer joins a set as any other does, holder or member. The dispatch's global buffer of pages
(D961) holds pages behind a table rather than a range, and a buffer on one of its pages, where
either is written, is refused.

**Why:** run 305 of Grand Theft Auto V refused three dispatches as two overlapping buffers. The
programs at `0x400003e19200` and `0x400003e1ac00` bind a 0x20-byte written range that a
0xe40f-byte written range ends inside. The program at `0x400003d88400` binds a four-byte written
range inside a 0x94-byte range. On the hardware each pair addresses the same bytes. Its program at
`0x74005b87e400` binds two sampled ranges of one descriptor, slot 20 a megabyte into slot 18, both
written.

**Rejected:**
- Two buffers, with the written one copied over the other afterwards. A load through one slot after
  a store through the other in the same wave would read the stale copy. D937 rejected this for the
  same reason.
- Placing the shared slots relative to a pushed base, as D951 places a sampled buffer. That treats
  every access through them as a possible escape, and refuses scalar loads through them.
