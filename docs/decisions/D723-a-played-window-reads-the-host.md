# D723 - a played window reads the host clock

**Status:** assumed
**Date:** 2026-09-25

## The question

The operator played Neverball in the GUI and it ran too fast. Measured headless: in 41 s of wall
time the port's once-a-second frame report printed 155 times. Game time ran about four times faster
than real time.

The clock was the logical one (D582). It advances a microsecond per reading and a sleep's length per
sleep. One of the port's threads spins on `sceKernelGetProcessTimeCounter`: about 100 million
readings in that run, about 100 s of logical time. `SDL_GetTicks` reads the same source, so
Neverball's fixed-step physics ran as far ahead as the spinning thread pushed it.

## The choice

**The GUI runs its workers on the host clock unless `ORBISTOUN_CLOCK` is set.** Everything else
keeps D582's logical default.

- **D582 is right for what it was for.** A run is a measurement, and a measurement has to repeat.
  The CLI, the loop and the corpus runs stay logical.
- **A window someone plays is not a measurement.** On the console a title's clock is real time. A
  person holding a pad judges the result by it, and a spin-wait elsewhere in the process does not
  speed it up. The logical clock gives no real guarantee of pace, and only the host clock does.
- **An explicit setting still wins.** `ORBISTOUN_CLOCK=logical orbistoun-gui` reproduces a headless
  run in the window, for instance to watch a recorded capture behave exactly as the test did.

A played-back capture stays in step either way: its steps are keyed to flips, not to time (D721).

## What it rests on

- D582's measurements, and its own statement that `host` is for "how long something really took".
- The 155-reports-in-41-s measurement above.
