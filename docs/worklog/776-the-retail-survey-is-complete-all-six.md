# 776. The retail survey is complete: all six titles at deep walls; ASTRO BOT is a null-object deref inside a C++ static constructor and, being native, is the most RE-tractable grind target

**2026-09-21** — this pass examined the one retail title not yet looked at, PPSA21564 (ASTRO BOT). Its
wall is understood and it is not a cheap fix, which completes the survey: every retail title now has an
exact, recorded blocker, and none is a single-tick move. This records the finish line and picks where a
sustained grind has the best odds.

## ASTRO BOT's fault

A clean run faults at `the title's own modules+0x7af792`, `read of 0x38`, with `rax=0x0` and `rdi=0x0` -
a field read at `+0x38` through a null pointer. The calls just before it are the tell:

```text
libc::__cxa_guard_acquire(...) -> 0x1     (first-time init: construct the static)
libkernel::scePthreadAttrSetaffinity/Setschedpolicy  (building a thread attribute block)
libc::__cxa_guard_release(...) -> 0x0     (void; the report's routed lead, worklog 606)
```

So the null is dereferenced **inside a C++ function-local static's constructor** (bracketed by the
`__cxa_guard` pair) that is setting up a thread. An object the constructor expects is null at `+0x38`.
Consistent with worklog 606: `__cxa_guard_release` is `void`, so the null came from *further back* - a
value stored or loaded earlier in the constructor, not the immediately preceding call.

## The survey, complete

Every retail title now has a recorded, exact blocker, and each is a sustained effort, not a loop-tick fix:

| title | wall | nature |
|---|---|---|
| PPSA04263 (GTA) | `image+0x19676d7` | input-manager-init never reached; deep startup RE (764-768, paused) |
| PPSA25872 (Terminator) | `image+0x17554a3` | fatal memory-pool alloc; Il2CPP metadata blocks static RE (771-775, paused) |
| PPSA02664 / PPSA03416 | `image+0x3f8f0` | AGC descriptor from inline non-exports; needs synthesis (723/729/770) |
| PPSA28061 | `image+0x10b9e9` | `sceAgcGetRegisterDefaults2`, a non-export; needs synthesis (769) |
| PPSA21564 (ASTRO BOT) | `own modules+0x7af792` | null in a C++ static ctor; native, RE-tractable (this) |

No wall here is a missing named import orbistoun can implement from a lawful source in one step - that
frontier was spent long ago (worklog 521). What remains is disassembly grinds and clean-room synthesis.

## Why ASTRO BOT is the grind to pick

Of the five deep walls, ASTRO BOT is the most tractable to disassembly, for three reasons: it is **native
C++**, so string-reference navigation works (the Il2CPP metadata-table obstacle that paused Terminator,
worklog 775, does not apply); it **runs the furthest** of the entered titles (500,260 calls); and it dies
on a **single null-object deref**, not a chain or a synthesis gap. A title that runs half a million calls
and stops on one null pointer is the closest thing the retail set has to a findable, fixable cause.

The grind: identify which shipped module holds `+0x7af792` (the fault is in module space `0x48…`, not the
eboot), disassemble the faulting constructor, and trace back to where the object that should sit at
`+0x38`'s base is created - and whether its null is an orbistoun value (a stubbed call that answered zero
where an object belonged) or the title's own path. Native code and working string-refs make that a real
prospect rather than the tooling-blocked dead ends the Unity titles hit.

## Gate state

No code changed - a survey-completion that records ASTRO BOT's null-in-a-static-ctor fault, tabulates all
six titles' exact blockers as sustained efforts, and commits the next grind to ASTRO BOT as the most
RE-tractable of them. `./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No
commit.
