# 795. PPSA04263's eboot has no `DT_INIT_ARRAY` (size 0), disproving the init-array-timing lead: the static object's constructor is runtime code orbistoun's execution never reaches, so it is the same execution-divergence wall as the other native titles, not a buildable global-constructor fix

**2026-09-22** — worklog 794 found PPSA04263's null vtable is a static bss object whose constructor
never runs, and raised a buildable hypothesis: if that constructor is a global constructor in the eboot's
`DT_INIT_ARRAY`, running the array at the right point clears it. This tick parsed the eboot's dynamic
section and disproved it.

## No init array

The eboot's `PT_DYNAMIC` carries **`DT_INIT_ARRAY = 0x0`, `DT_INIT_ARRAYSZ = 0`** - zero constructors -
and no `DT_PREINIT_ARRAY`. So there are no global constructors run through the init-array mechanism at
all, and `image+0x5b37e98`'s constructor is not one of them. The "run the eboot's init array at the right
time" fix has nothing to run: orbistoun already runs no init array here because this image declares none,
not because it skips one.

## What that leaves

A static object whose bytes are never written, with no global constructor to write them, means the
construction is **runtime code** - a lazy singleton behind a guard, or a subsystem-init call made during
`main` - that this run's execution never reaches. That is exactly the execution-divergence signature the
other native titles carry (ASTRO BOT's arena-init, GTA's prior walls): the guest runs tens of thousands
of calls but does not reach the specific construction, and finding where it diverges needs the execution
trace from entry, not a static list. The static-vs-heap distinction (794) was real and worth having - it
ruled the object out as a failed allocation - but it does not make this title buildable the way an
init-array entry would have.

## The honest state, restated

Eleven ticks past worklog 787's region-return win, every retail title is confirmed tool- or
hardware-bound from multiple angles, and the one buildable-looking exception this session (init-array
timing) is now closed:

- Three native/Il2Cpp titles: runtime construction not reached, computed/execution-divergence
  (tracer-bound).
- Two Unity-AGC titles: workload-array state (obSCEne REQ-...7a5d, filed and sharpened).
- One AGC title: hardware-faithful mapper (obSCEne a2f9), or populated register defaults.

The single-tick loop's wall-moving moves are exhausted; the remaining progress is an enabling build (an
execution/branch tracer, as 787's region-return was a small build) or a pending hardware measurement.
The productive work the loop can still land without either is honest fidelity - real exports answered
from published semantics rather than a placeholder - which is where it turns next rather than
re-confirming the same walls.

## Gate state

No code changed - a disproof that reads the eboot's `DT_INIT_ARRAY` as empty, closing the
global-constructor-timing lead and returning PPSA04263 to the execution-divergence class. `./bin/orbistoun
check` green, worklog index regenerated, identity scan clean. No commit.
