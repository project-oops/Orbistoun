# D813 - a signal raised on a running thread waits for its next wait

**Status:** decided
**Date:** 2026-10-12

`sceKernelRaiseException` on another thread is accepted whatever that thread is doing, and answers
0, as the hardware answers it. The signal waits on the target until the target is inside a wait
orbistoun owns (an address or umtx wait, a lock, a semaphore, a condition variable, an event
flag): at once if it sleeps in one now, or as it enters the next one, where the wait runs the
handler and then waits on (D652, D808). A handle orbistoun never issued is refused as before.

This replaces D652's refusal of a raise on a thread that is not asleep. That refusal answered a
signal the hardware would have delivered with the placeholder. A collector that ignores the
answer, as IL2CPP's does, then waits forever for an acknowledgement the target never sends.

A target that never enters such a wait never runs the handler, which is no worse than a refusal:
in both cases its collector waits.

**Why:** PPSA25872's IL2CPP collector raised the suspend signal on its main thread while main was
running guest code. The raise was refused, the collector waited on "SuspendSemaphore" holding the
GC lock, and main then went to sleep on that same lock without ever acknowledging (D806, D807
named the waits). The title made no progress after its loading screen.

**Rejected:**
- Running the signal at main's next import call instead of its next wait: a call is not a point
  where the handler's acknowledgement and wait for resume are safe, while an owned wait is one by
  construction (D652).
