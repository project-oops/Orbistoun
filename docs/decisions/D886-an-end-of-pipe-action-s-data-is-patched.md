# D886 - An end-of-pipe action's data is patched where radeonsi emits it

**Status:** decided
**Known by:** assumed
**Date:** 2026-10-15

`sceAgcQueueEndOfPipeActionPatchData(packet, data)` writes `data` into the `RELEASE_MEM` packet a
builder reserved, as the value its end-of-pipe action stores: the low half into dw5, the high half
into dw6, the rest of the packet as built. It answers `0x0`.

**Why:** Grand Theft Auto V calls it once per submission, with values 1 and 2, on the
`RELEASE_MEM` its `sceAgcCbReleaseMem` reserved: the fence each frame's end-of-pipe action writes.
Answered with the placeholder, every release stored the value the packet was built with, and the
fence never counted.

dw5 and dw6 are where radeonsi emits an end-of-pipe write's immediate data, after the destination
address in dw3 and dw4 (Mesa `src/amd/common/ac_cmdbuf_cp.c:494-501`). The destination half is
measured: `sceAgcQueueEndOfPipeActionPatchAddress` replaced dw3 and nothing else
(`166-agc/patch-queue-eop-address`) and answered `0x0`, which is the answer assumed here.

**Rejected:**
- Leaving it unimplemented until measured: the call is GTA V's once per frame, and its packet
  layout is radeonsi's, which every measured `RELEASE_MEM` field so far has matched.
