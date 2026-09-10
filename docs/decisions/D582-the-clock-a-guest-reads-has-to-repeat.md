# D582 - The clock a guest reads has to repeat, and still move

**Status:** measured
**Date:** 2026-09-08

## The bug, stated as the project's own requirement

Two runs of one build placed the same reservation at `0x7400047e0000` and `0x740004830000`, and
a pointer PPSA03416 hands to `sceKernelAprSubmitCommandBufferAndGetResult` alternated between
`0x740009100000` and `0x740009200000` across four runs. D181 and D238 require every measurement
here to survive a repeat. This one did not, and it is why the buffer at that pointer had to be
read by typing an address out of a previous run and hoping (D580).

Two things orbistoun handed the guest were different on every run.

## A thread handle was a host heap address

`next_handle` returned `Box::leak`, so `scePthreadSelf` answered `0x23b5e3ade80` one run and
`0x22841ab0610` the next. D151 established that a handle must be a **real, aligned, zeroed block
the guest can dereference** - an opaque integer reproduced a fault at a low address - and
nothing in that reasoning requires the host allocator to choose where.

So the blocks come from `CONTROL_BLOCK_BASE`, in the `0x0000_5E2*` family
`docs/ADDRESS_MAP.md` keeps for regions of orbistoun's own invention, bump-allocated in order of
registration. Handle *n* is the same address in every run.

**The fallback is not silent.** A run whose reservation failed uses the host heap again and has
lost the property, so `handles_repeat` says which happened rather than leaving a reader to infer
it from addresses that look equally plausible either way.

## The clock was the host's, and D256 had decided that deliberately

> **Not the same clock as the run's call budget.** That one exists to make a run reproducible;
> this one is a value the guest reads and branches on, and pinning it would stop any title that
> waits for time to pass (D256).

That is right about pinning and it is not the only option. **A clock that repeats does not have
to be a clock that stands still.** One that advances by a fixed step per reading does both, and
that is what D256 did not have in front of it - so this supersedes its conclusion rather than
its reasoning.

One microsecond per reading: small enough that a guest timing its own work reads a plausible
number, large enough that a spin-wait on a millisecond ends in a thousand readings rather than a
million. A sleep advances it by what was asked for, so waiting for real durations still works.
The wall clock is a fixed, obviously synthetic epoch plus the same elapsed time, so a guest that
formats a date gets the same string every run.

**Default `logical`, `ORBISTOUN_CLOCK=host` for when the question is how long something really
took.** The default is the one that makes a run comparable, because a measurement that cannot be
repeated is not one.

### One source, or the fix is the half that looks like a fix

`orbistoun-kernel` kept its own `Instant` for `GetProcessTime` and another for the tick counter,
so the platform's process time, its counter and POSIX's monotonic clock were three clocks. All
three read `clocks::since_start_nanos` now. A guest converting between them lands where it
expects, and one setting decides whether all of them repeat.

The same hazard bit the sleeps: `orbistoun-libc`'s three were taught to advance the clock and
`orbistoun-kernel`'s `usleep` was not, which is the wiring hazard this project keeps paying for.
The libc three go through one `slept`.

## Measured

Two runs of PPSA03416, comparing the mapping sequence (D581):

| | identical mappings |
|---|---|
| `ORBISTOUN_CLOCK=host` | **1** of 49 |
| logical clock | **19** of 47 |

The wall is unchanged - 193 imports, the same fault, the same one read of zero bytes - which is
the correct outcome for a change that alters what the guest is told and not what it is given.

## What is left, and it is not fixable here

The divergence now begins **exactly at the first `scePthreadCreate`**, which the mapping record
names. Guest threads are real host threads - principle 6, and the reasons are not negotiable -
so their interleaving is the host scheduler's, and two runs will not agree about the order two
threads reach an allocator.

Reproducing that would mean scheduling guest threads deterministically, which is a different
emulator. **What is achievable is that everything before the first thread repeats**, and it now
does, address for address and call for call.

## What this does not establish

**That the run is deterministic.** It is deterministic up to the first thread and not after it,
which is a real improvement and not the property. A title that maps everything before spawning
would be fully repeatable; this one does not.

**Nor that the logical clock is right for every guest.** A title that measures how long its own
work took now reads a number derived from how many times it asked, not from any real duration.
Nothing observed depends on that, and a title that did would need `host` and would then be
unmeasurable - which is a trade to make knowingly rather than a property to rely on.

**Nor that these were the only two host values reaching the guest.** They are the two that were
found by diffing a mapping sequence. The others, if any, would surface the same way.
