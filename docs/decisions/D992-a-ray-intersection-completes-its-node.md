# D992 - A ray intersection completes its node tests

**Status:** decided
**Date:** 2026-10-25
**known_by:** assumed

`image_bvh_intersect_ray` and `image_bvh64_intersect_ray` test every node kind the pointer's type
bits name. They honour every field of the descriptor the instruction set lists (AMD's published
RDNA 2 instruction set, table 49). The test is written once, over `Arithmetic`, for the translator
and the host evaluator alike (`model::bvh`). Grand Theft Auto V's traversal pass reaches only part
of it (D991). The rest follows the instruction set's fields and the technique the guest's own nodes
show. Where the instruction set does not say what a node answers, the choice is marked assumed.

**Triangle nodes.** A triangle node holds five vertices, three words apart from word 0. Type bits
0 to 3 name one of four triangles over them: (0,1,2), (1,3,2), (2,3,4) and (2,4,0). Each triangle
after the first shares an edge with one before it and is wound as the first. Word 15 holds the
node's flags, a byte a triangle.

- Bits 1:0 and 3:2 of a triangle's byte say which of its vertices' weights return mode 1 answers
  first and second. Every node the guest builds holds `9` there: the second and third vertices'
  weights, the usual barycentrics.
- Return mode 0 answers the flags word whole, as the triangle's id, and a hit flag.
- A miss answers an infinite `t` over a `det` of one, with the same last two words and a hit flag
  of zero.
- The extent is not applied to a triangle. The shader compares `t / det` with it, as Grand Theft
  Auto V's does.

Before this, every triangle was the first, the id came from word 12, mode 0's flag also checked
the extent, and a miss answered zero weights. Assumed: the third and fourth triangles' vertices,
and mode 0's id. No guest seen builds them or uses that mode.

**The hit distance's sign.** `t` is the weights' sum weighted by each vertex's sheared depth, and
`det` the weights' sum. A hit's two share a sign: positive where the triangle is wound
counter-clockwise as the ray sees it, negative where clockwise. The instruction culls nothing; a
shader culls a facing by that sign. Grand Theft Auto V's traversal compares bit 31 of `t` with a
per-instance facing bit. Its pass at `0x74005b7e8600` had culling off in the frame captured.

**Box nodes.**

- A box of floats is 128 bytes and a box of halves 64. Their bounds read as before.
- The descriptor's growth (bits 62:55) pushes each box's exit out by that many float steps before
  the test.
- Box sorting (bit 63) orders hits by where the ray enters, from its origin for a box it starts
  inside. A sorting network keeps the node's order between equal distances, and misses come last.
- Unsorted, hits keep the node's order and misses answer `0xffffffff` in place.

**The descriptor's bounds.** A node whose last 64-byte slot - the second for a box of floats - lies
past the descriptor's last node index (bits 105:64) answers `0xffffffff` in every word. So does a
node read through a descriptor whose type (bits 127:124) is not a hierarchy's (8). Instance and
procedural nodes answer the same, as before. What a node past the bounds answers is assumed. Grand
Theft Auto V's descriptors hold type 8 and a last index its nodes keep within.

**Tests.** `model::bvh`'s unit tests use values worked out by hand from the geometry, the sheared
vertices, their weights, `det` and `t`, written out in each test's comment:

- the four triangles of one node, each hit alone;
- the flags byte reordering the weights;
- mode 0's word;
- a triangle seen from behind answering `-32` over `-16`;
- the miss words;
- a box exit grown by one step, turning a miss into a hit;
- sorting from the origin;
- the descriptor's bounds and type.

Grand Theft Auto V's traversal replays to the same output as before.
