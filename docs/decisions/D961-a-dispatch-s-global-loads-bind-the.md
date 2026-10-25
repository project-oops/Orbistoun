# D961 - A dispatch's global loads bind the pages they read

**Status:** decided
**Date:** 2026-10-25
**known_by:** guest-observed

A dispatch's global loads and global stores with no scalar base reach one buffer that holds only the
guest pages they reach, each whole, behind a hash table of the pages' full 64-bit numbers. Word 0 is
the table's size as a power of two; each slot holds a page's number, low half then high, and where
its words start, zero for an empty slot; the pages follow. The module hashes an address's page
number to its slot and searches it and the next, and the host grows the table until every page lies
there. An access to a page the buffer does not hold is an escape (D950), and a store there changes
nothing. Where every wave ran (D883) the pages are those an access reached a byte of; a sample
(D950) holds every page of its range and margin. The stores' changed words are written back to
their own pages, found through the table. A buffer on a held page is refused where either it or the
global buffer is written. The pages and table are bounded at 256 MiB; the span between pages is not.
Only whole-word stores are traced; a store of part of a word leaves the program untraced (D746).

**Why:** Grand Theft Auto V's compute program at `image+0x3d14100` follows each element's pointer to
an object, and its objects lie 1.5 GB apart around the buffer it writes: bound as their span,
0x5a079604 bytes, the dispatch was refused; bound as the pages read, it is 97 pages, about 2 MB. Its
program at `0x74005b87e400` reads 363 pages across 18 GB, which a table indexed from the first page
could not span. Its programs at `0x74005b854a00` and `0x74005b857200` store their results with
`flat_store_dword` beside their buffer loads, which D746 left untraced.

**Rejected:**
- Binding the span: gigabytes per dispatch, and it overlaps written buffers.
- A dense table from the first page: its size follows the span, not the pages held.
- A sorted table searched per access: a binary search unrolled for every lane's word grows the
  module by its depth; two probes of a hash table bound the work.
- One buffer per cluster of objects: needs dynamic binding indices, and the number of clusters is
  unbounded.
- Tracing byte and half stores: two lanes' stores into one word would race on the device.
- Refusing the dispatch: its result never lands.
