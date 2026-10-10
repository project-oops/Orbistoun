# D786 - A flip wakes its waiter at the display's time

**Status:** decided
**Date:** 2026-10-10
**known_by:** assumed (the refresh: 60 Hz, one refresh per flip; `sceVideoOutSetFlipRate`'s rates
and the output's refresh are unmeasured)

A flip completion carries the display's logical time: the port's flip count times one 60 Hz
refresh. A thread whose `sceKernelWaitEqueue` takes it moves its logical clock up to that time.

That time travels on through the two primitives a title's threads hand a frame along with. A
semaphore remembers the latest time any thread signalling it had, and a thread that takes from it
moves up to that time. An event flag remembers the latest time any thread setting it had, and a
waiter whose pattern it matches moves up to that time. A clock only ever moves forward. Mutexes,
condition variables and every other wait are as D735 has them. Under `ORBISTOUN_CLOCK=host`
nothing changes.

This amends D735's per-thread clock. A thread that waits on the display, or on a thread that
waited on it, sees a frame's time pass per frame, as it would watching a real display.

**Why:** Unity's main thread never waits on the display itself. It trades semaphores with
`UnityGfxDeviceWorker`, which waits on event flags the flip-waiting thread sets. Under D735 the
main thread's clock read about 38 ms after a minute of running, and PPSA03416 never left its boot
scene; under the host clock it opened `level1`. With time carried along that chain it opens
`level1` under the logical clock too. The user chose each step on 2026-10-10: flip waits first,
then semaphores, then event flags, as each proved insufficient alone.

**Limits:** this is close to the Lamport merge D735 rejected, for the same reason D735 gave: where
several threads signal one semaphore or set one flag, the time a waiter takes depends on which
signal it was released by, and the host's scheduler decides that. A frame handed from one thread
to one other repeats; a contended primitive may not.

**Rejected:**
- The host clock for commercial titles: runs stop repeating at all.
- Carrying time through mutexes and condition variables as well: not needed for the chain
  measured, and every further primitive widens what the host's scheduling decides.
