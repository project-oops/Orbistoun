# D843 - The index offset is added to every vertex index

**Status:** decided
**Date:** 2026-10-14
**known_by:** assumed

`GE_INDX_OFFSET` (uconfig `0xC24A`) is added to every vertex index before it becomes a vertex id.
A non-indexed draw (`DRAW_INDEX_AUTO`) of `n` vertices under offset `k` has ids `k` to `k + n - 1`,
and its primitive shader is seeded so. An indexed draw adds `k` to each index it fetches;
orbistoun does not yet offset fetched indices, so an indexed draw with a non-zero offset stays
refused, naming the offset.

**Why:** Mesa's r600 driver programs the same register under its older name, `VGT_INDX_OFFSET`.
For a non-indexed draw it writes the draw's `start` there and emits a plain `DRAW_INDEX_AUTO` of
`count` vertices (`r600_state_common.c` 2697, 2939). For an indexed draw it writes the index bias,
the base vertex (2695). Both work only if the hardware adds the register to the generated index
and to the fetched one alike. radeonsi writes zero on gfx10 and later and passes the base vertex
in an SGPR instead, so no newer driver shows the register used.

Grand Theft Auto V's first draw is a three-vertex `DRAW_INDEX_AUTO` under offset 5. Its render
thread waits on the `RELEASE_MEM` fence behind that draw (D832, the queue's held writes).

**Assumed:** that the register kept its r600 meaning through the rename to `GE_INDX_OFFSET`.
obSCEne `REQ-20261014T0415Z-ix01` measures it on the console, for an auto draw and an indexed one.

**Rejected:**
- Seeding ids from 0 under any offset: a full-screen pass reading vertices 5 to 7 would read 0
  to 2.
- Waiting for ix01 with the title stopped: the meaning is grounded in the driver that uses the
  register, and the measurement confirms or retires it.
