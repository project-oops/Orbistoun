# 422. The stack the collector scans

**2026-09-07** - directed, continuing 421

## What was done

**Found the wall three titles shared, and it was two pthread calls.** PPSA25872, PPSA02664 and
PPSA03416 all died in the first unmapped page above the main stack, all share the `PS5Util`
module, and all call `scePthreadAttrGet` then `scePthreadAttrGetstackaddr` once, unimplemented -
the sequence a garbage collector runs to find the bottom of the stack it scans. With a
placeholder for the address, the bottom was arithmetic on garbage and the scan ran off the top
(D575).

Implemented both on FreeBSD's `pthread_attr_get_np` and `pthread_attr_getstackaddr`: the thread
record now carries the stack a spawned thread reserved, the entered thread answers with the span
the worker placed, and a thread with neither is refused. One test pins the answer and the refusal.

| Title | before | after |
|---|---|---|
| PPSA25872 | 121 imports, crash | **141**, call budget - FURTHER |
| PPSA02664 | 119, crash | **197**, its record - FURTHER |
| PPSA03416 | 119, crash | **186**, record 187 - FURTHER |

**That closes 421's open regression.** The records were never ahead of the code; with thirteen
threads spinning, whether the main thread reached its first collection inside the limit was
scheduling luck. Modelled, it reaches it every time.

Two experiments were run first and ruled out: the reference machine profile and a relative
module path both reproduce the crash exactly.

## Boundary kept

A read of the loader's process-description builder was refused, and the investigation stopped
there. What was needed turned out to be in the trace and the kernel crate anyway - which is
worth noting, because the first three hypotheses (argv, profile, initial-stack layout) were all
about the loader and all wrong.

## Surprises

**A title's own export lands on a stub.** PPSA25872 now spins 19.7 million times on
`PS5Util::0xf948d02a4f9f5ace`, and `PS5Util.prx` exports exactly that hash at `+0x2a0`, beside
the other one the executable imports from it. D484 binds a title's own exports ahead of the stub
table; these two are not bound. The linking step was not opened. This is the next wall for the
title, and it is 98% of its calls.

**`sceKernelMprotect` on module memory answers `EINVAL`.** PPSA03416 asks for 256 MiB of write
access from inside its own module; the range is not a mapping the kernel crate owns, so it is
refused as the console refuses an invalid mapping.

**Corrected the same day (D576).** This entry went on to say the guest then jumped into the
stub table off a thunk's start, and named the next step as a linking question. That came from
a fault message which called every breakpoint stub padding without looking at the address -
the stub table does not cover it, and the trap is in the title's own module code. The
`EINVAL` is measured not to be the wall either: forced to answer success, the fault does not
move. **The reading is withdrawn and so is the linking work it pointed at.**

**PPSA02664's next wall is the GPU's**: `sceAgcDriverAddEqEvent` unimplemented, then a poll on
the event queue that can never deliver, then a read of `-1`.

## Next

- The unbound `PS5Util` exports - a linking question, for a session allowed to open it. This
  is PPSA25872's wall and, per D576, **not** PPSA03416's.
- What PPSA03416 checked immediately before it trapped. A breakpoint in the guest's own code
  is an assertion, a deliberate trap, or a jump into data, and a fault record cannot tell
  them apart - so the preceding calls are where to look.
- `sceAgcDriverAddEqEvent`, on the Agc side.
- A hardware probe reading `scePthreadAttrGetstackaddr` and `Getstacksize` on the main thread,
  to settle whether the address is the lowest byte.
