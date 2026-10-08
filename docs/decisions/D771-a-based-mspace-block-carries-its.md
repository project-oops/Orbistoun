# D771 - A based mspace block carries its dlmalloc header

**Status:** decided
**Date:** 2026-10-08
**known_by:** published (dlmalloc 2.8's chunk layout)

Every block an mspace created over a base hands out (D751) has dlmalloc's chunk header in the 16
bytes in front of it: `prev_foot` zero, then `head`, which is the chunk size (the block's rounded
length plus the header) with `CINUSE_BIT | PINUSE_BIT` set. The header lies inside the arena, and
a block's span includes it, so the arena's in-use and peak counts include it too, as dlmalloc's
statistics count whole chunks.

**Why:** PPSA21564's allocator wrapper reads the word in front of every block its Onion mspace
returns. It masks the low four bits, takes `0x10` off, and adds the result to its own count of
bytes in use. D751 kept every byte of bookkeeping host-side, so the first block began at the
arena's base, and the read before it faulted outside the region.

**Supersedes:** D751's "nothing is written into the guest's region but what the guest writes".
The free spans and live map stay host-side; only the header a guest reads is written.
