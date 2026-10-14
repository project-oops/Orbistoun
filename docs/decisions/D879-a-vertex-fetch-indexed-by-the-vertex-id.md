# D879 - A vertex fetch indexed by the vertex id binds only the records the draw can index

**Status:** decided
**Date:** 2026-10-14

A draw buffer read through a descriptor with a record index (D733) binds from the descriptor's base
to the end of the last record an access can reach. Until now that was the descriptor's last record,
`records - 1`. It is now the last record the draw can index, when the translator shows where the
index comes from. The range still starts at the descriptor's base, so neither the module nor the
draw's places (D747) change.

The translator follows the vector registers of a primitive shader alongside the scalar ones it
already traces (D733). At entry `v5` holds the vertex id and `v8` the instance id (D863). A
`v_mov_b32` copies what a register holds. A `v_add_nc_u32` of an id and a scalar register that holds
a traced word records the id plus that word: a user-data word, a constant, or a word loaded from a
table. Any other write to a vector register forgets it, as do branches that disagree. An id written
to the local data share and read back is not followed: the trace cannot show which address of the
share holds it. An indexed access whose index register holds one of these marks its reads with the
source. Every indexed access through the same descriptor must agree, or none of them bounds the
range.

The draw finds the bound. A non-indexed draw's vertex ids run from its first vertex through its
vertex count; a split draw's (D741, D787) run through its last chunk. Its instance ids run from zero
through its instance count. The added word is resolved for the draw as the descriptor's words are.
The last record is then the last id plus the word, capped at `records - 1`. An index the hardware
would wrap past 32 bits keeps today's range. An indexed draw also keeps today's range: its vertex ids
are the index buffer's values, which nothing here reads.

This narrows what D733 and D758 bind and nothing else: an access with no traced index source, a
dispatch, a fragment shader and a raw or unchecked descriptor all bind what they did.

**Why:** MCUB00001 (Mesa Cube, oops-mesa) spent 76% of its run reading draw buffers, about 17,000
reads of 842 us each. Mesa points every vertex buffer descriptor from the draw's base to the end of a
4 MB upload ring. Each draw's range came out 4.19 MB, then 3.46, 2.74, 1.99 and so on, a new key
every draw. The buffer cache never hit, and each draw copied and hashed megabytes to read a few
kilobytes. Its fetch is `v_add_nc_u32 v5, s13, v5` and then `buffer_load_format_xyzw v[0:3], v5,
s[20:23], 0 idxen`: the vertex id plus the base vertex in user data. That is all the draw can index.
Its culling shader's second fetch reads the id back from the local data share after compaction, so
that one buffer keeps its whole range.

**Rejected:**
- Refreshing a kept range page by page: the keys never repeat, so nothing is kept to refresh.
- A cache of pages that assembles each range: assembling a range cost as much as reading it.
- Reading the index buffer for an indexed draw's largest index: a scan per draw for draws this title
  does not issue. It can be added beside this if an indexed title needs it.
- Running the program per draw to find the indices (D758's approach): the index is an id plus a
  word, which the trace states exactly, so no run is needed.
