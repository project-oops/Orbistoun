# 865. Neverball ran four times fast in the window

**2026-09-25**. The operator reported Neverball playing "too fast" in the GUI.

**Measured.** The port prints a frame report each time `SDL_GetTicks` passes another second.

| clock | wall time | reports |
|---|---|---|
| logical (the default) | 41 s | 155 |
| `ORBISTOUN_CLOCK=host` | 40 s, load included | 38 |

**Cause.** Under D582's logical clock every reading advances time by 1 µs. A port thread spins on
`sceKernelGetProcessTimeCounter`, about 100 million readings in the logical run. Every guest clock
reads the one source, so game time was driven by that spin rather than by wall time. The
fixed-step physics (90 Hz, catch-up capped by the port's patch 0004) followed it.

**Change (D723).** `orbistoun-gui` sets `ORBISTOUN_CLOCK=host` for its workers unless it is already
set. Headless runs stay logical, so they still repeat.

**Open.** The counter spin itself: 100 million calls in 40 s is a busy-wait somewhere in the port or
its SDL backend. It costs host CPU, not correctness under the host clock.
