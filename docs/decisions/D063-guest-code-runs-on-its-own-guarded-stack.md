# D063 - Guest code runs on its own guarded stack

**Status:** decided
**Date:** 2026-08-19

Guest code runs on a dedicated stack with a guard page below it, reserved as one span, and is
entered through a stack switch. Entry is refused unless the entry point lies in an executable
segment.

**Why:** a guest overrunning a borrowed host stack corrupts host frames and faults inside the
emulator. A guard page turns the overrun into an immediate fault at an adjacent address.
Jumping to non-executable memory is certain to fault, and the refusal names the problem where
the fault would not.

**Rejected:**
- Running on the host thread's stack: overruns corrupt the emulator.
- A separately reserved guard: something could be placed in the gap.
