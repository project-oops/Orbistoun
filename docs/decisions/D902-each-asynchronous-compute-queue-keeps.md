# D902 - Each asynchronous compute queue keeps its own order

**Status:** decided
**Date:** 2026-10-19

`sceAgcDriverCreateQueue` gives each queue an object of its own, and `sceAgcDriverSubmitAcb(queue,
descriptor)` records which queue each compute submission was handed to. A compute stream stepped
aside at a wait (D891) holds back only the streams later handed to its own queue. Another queue's
streams run on.

When the stepped-aside streams are resumed, each queue's first stream is run in turn from where it
stopped, until it waits again. The round repeats while any stream gets further, because a write by
one may satisfy another's wait. The graphics queue stays one ordered ring (D710): its submit names no
queue.

**Why:** Grand Theft Auto V's graphics queue waits on a label that its second and third compute streams
write. Those streams were held behind its first compute stream, which waits on a later graphics
submission: a cycle the console does not have, because the compute streams are on separate queues.
With each queue in its own order, the label is written and the graphics queue runs on.

**Rejected:**
- Running every compute stream past any wait: one queue's streams depend on one another's order as a
  graphics ring's do.
