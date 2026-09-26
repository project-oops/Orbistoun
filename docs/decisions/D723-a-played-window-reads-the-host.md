# D723 - A played window reads the host clock

**Status:** assumed
**Date:** 2026-09-26

The GUI runs its workers on the host clock unless `ORBISTOUN_CLOCK` is set; the CLI, the loop and
corpus runs keep D582's logical default.

**Why:** a run is a measurement and has to repeat, which is what the logical clock is for. A window
someone plays is not a measurement: on the console a title's clock is real time, and a spin-wait
elsewhere in the process does not speed it up. The logical clock advances a step per reading, so a
guest thread spinning on `sceKernelGetProcessTimeCounter` runs game time several times faster than
real time. An explicit setting still wins, so `ORBISTOUN_CLOCK=logical` reproduces a headless run in
the window; a replayed capture stays in step either way, because its steps are keyed to flips (D721).

**Rejected:** the logical clock in the window - it gives no guarantee of pace to a person holding
a pad.
**Rejected:** the host clock everywhere - measured runs stop repeating.
