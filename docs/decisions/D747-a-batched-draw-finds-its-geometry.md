# D747 - A batched draw finds its geometry buffers through its draw data

**Status:** decided
**Date:** 2026-10-06

A primitive shader's buffers (D733) are no longer bound one per slot in a set of their own. Every
geometry buffer is a range of one of the shared arenas the backend uploads draw buffers into; the
geometry binding of the draw-buffer set holds the arenas themselves, and each draw's words in the
draw-data buffer (D718) say, for every slot, which arena, from which word and how many. The module
reads a slot through those three words. The pixel stage's buffers keep their binding as before.

**Why:** a batch is one host dispatch of many guest draws, and it can hold only draws that bind the
same set. With a set per draw's buffers, a frame whose draws each bring their own vertex buffer never
batches: CRFT00001's 1.5 million draw joins in 90 s all failed on the buffer set and nothing else,
and each of its ~34,000 draws a frame ran as a one-workgroup dispatch of its own. With the buffers
named in the draw data, every draw over the same pipeline, target and pixel buffers shares one set.

**Rejected:**
- A descriptor array indexed per draw: thousands of descriptors per set, past what a device without
  descriptor indexing binds.
- Buffer device addresses: an extension the remaining path does not otherwise need.
