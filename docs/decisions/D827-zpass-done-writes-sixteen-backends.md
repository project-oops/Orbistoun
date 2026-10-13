# D827 - ZPASS_DONE writes sixteen backends' counters, the count in the first

**Status:** assumed
**Date:** 2026-10-13

A draw counts its samples while `DB_COUNT_CONTROL` has `ZPASS_ENABLE` set and
`ZPASS_INCREMENT_DISABLE` clear. The pipeline sends that state as `SetSampleCounting`, the way it
sends the blend state. The Vulkan backend wraps each counting batch, or each counting draw outside
a batch, in a precise occlusion query from the open pass's pool. The pool is reset before the pass
begins, and the results are summed into one running count when the device settles.

The command processor carries out `EVENT_WRITE` `ZPASS_DONE` (event type 21). It settles the
device, reads the count, and writes sixteen 64-bit slots sixteen bytes apart at the destination,
each with bit 63 set. The first slot holds the count; the other fifteen stay at zero. A device
without `occlusionQueryPrecise` counts nothing exactly, and the event stays a stop.

**Why:** measured on the console (obSCEne `-4d19`), a `ZPASS_DONE` writes all sixteen backends'
slots with bit 63 set, and the sum of each slot's end minus its begin equals the pixels drawn.
oops-gl reads a query that way, and so does any reader that sums the backends. The split among
backends follows the console's screen tiling, which is not measured, so it is assumed: only the
sum is exact. Without this a GL title's occlusion query stopped the command processor, and every
frame after it waited.

**Rejected:**
- Splitting the count across the backends: a split nothing measured would look like data.
- A query per draw: oops-gl's default `DB_COUNT_CONTROL` counts in every frame, and a query per
  draw would end every batch.
- Answering zero, or the draw's area: neither is what the depth test passed.
