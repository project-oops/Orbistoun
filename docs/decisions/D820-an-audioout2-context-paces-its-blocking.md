# D820 - An AudioOut2 context paces its blocking pushes by a queue of grains

**Status:** assumed
**Date:** 2026-10-12

A context keeps a queue of grains, drained one per grain of time from the moment a grain is
queued. `sceAudioOut2ContextPush` queues one: a blocking push (flag 1) with two queued first
waits for one to drain, a non-blocking push (flag 0) never waits and the queue saturates at
three. `sceAudioOut2ContextGetQueueLevel` reports the grains queued and the available count
measured beside each: 2 at 0 and at 1, 1 at 2, 0 at 3. Nothing is queued, and no push waits,
until a port has published PCM through `sceAudioOut2PortSetAttributes`. A grain is the context
parameters' fifth word in frames at 48000 Hz.

**Why:** measured (obSCEne REQ-ap02, `report-1791842051.txt` 50-385): on PPSA04263's
parameters (fifth word 0x100) with a 2-channel port publishing PCM, blocking pushes returned at
once twice, then each a 256-frame grain (5.33 ms) after the last, 189 in a second; non-blocking
pushes saturated at 3 queued. With nothing published (REQ-24b2) blocking pushes never waited.
Unpaced, PPSA04263's and PPSA21564's audio threads spend a run's whole call budget.

**Assumed:** the fifth word as the grain on other parameter sets (PPSA21564's is 0x200), the
drain clock starting at the first queued grain, and the 10 ms the first measured wait took
(not reproduced: it waits one grain).

**Rejected:** pacing every push regardless of publication (REQ-24b2 measured none); answering at
once (the measured pump paces).
