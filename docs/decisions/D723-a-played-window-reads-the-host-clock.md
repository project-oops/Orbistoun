# D723 - A played window reads the host clock

**Status:** assumed
**Date:** 2026-09-26

The GUI runs its workers on the host clock unless `ORBISTOUN_CLOCK` is set; the CLI, the loop
and corpus runs keep the logical clock by default.

**Why:** a run is a measurement and has to repeat, which is what the logical clock is for. A
window someone plays is not a measurement: on the hardware a title's clock is real time, and the
logical clock advances a step per reading, so a guest thread spinning on the time counter runs
game time several times faster than real time. An explicit setting still wins, and a replayed
capture stays in step either way because its steps are keyed to flips (D721).

**Rejected:**
- The logical clock in the window: no guarantee of pace to a person holding a pad.
- The host clock everywhere: measured runs stop repeating.
