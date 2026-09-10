# 428. The determinism bug had two halves and a third that is architecture

**2026-09-08** - directed, continuing 427

## What was done

**Built the record that could find it.** A run reported the reservations that *failed* and
nothing about the eighty that succeeded, so two runs whose arena addresses differed could be seen
to differ and not where. `ORBISTOUN_TRACE_MAPS` lists every mapping in order, with the call
ordinal and the import that was running (D581). Each of those fields was added because the
previous diff could not answer the next question.

**Fixed two host values reaching the guest** (D582):

- A thread handle was `Box::leak`, so `scePthreadSelf` answered a different address every run.
  Handles now come from `CONTROL_BLOCK_BASE`, bump-allocated in registration order.
- The clock was the host's. It is now logical by default - a fixed step per reading, advanced by
  what a sleep asked for - so it repeats *and* still moves. `ORBISTOUN_CLOCK=host` restores the
  old behaviour.

D256 had decided the clock deliberately, and was right that pinning it would stop any title
waiting for time to pass. A clock that repeats need not stand still; that option was not in front
of it.

## Measured

Two runs of PPSA03416, comparing the mapping sequence:

| | identical mappings |
|---|--:|
| `ORBISTOUN_CLOCK=host` | **1** of 49 |
| logical clock | **19** of 47 |
| logical, ignoring call ordinals | 24-30 of 47 |

The wall is unchanged - 193 imports, `image+0x1389269`, one read of zero bytes - which is right
for a change to what the guest is *told* rather than what it is given. PPSA02664 holds at 198
imports and PPSA25872 at 140, so nothing regressed.

Across three titles, two runs each, mapping sequences compared:

| title | identical | of |
|---|--:|--:|
| PPSA04263 | **5** | 5 |
| PPSA02664 | 18 | 56 |
| PPSA03416 | 19 | 47 |

**PPSA04263 is now fully repeatable** - it walls before it makes a thread, so there is nothing
left to be non-deterministic about. The other two diverge where they spawn.

## Where it stops, and why that is not a shortfall

The divergence now begins **exactly at the first `scePthreadCreate`**. The record named it:
mapping 19 in one run reads `during libkernel::scePthreadCreate`.

Guest threads are real host threads (principle 6), so two runs will not agree on the order two
threads reach an allocator. Reproducing that means scheduling guest threads deterministically,
which is a different emulator. Everything before the first thread now repeats exactly.

## Surprises

- **The guest creates threads and no report said so.** `scePthreadCreate` is implemented, and the
  ranked findings only list what is *not* - so a fact that changes how every measurement should
  be read was invisible in the default output. The mapping record found it by accident, looking
  for something else.
- **Three clocks, not one.** `orbistoun-kernel` kept its own `Instant` for `GetProcessTime` and
  another for the tick counter, beside `clocks`. Converting only one would have been the half-fix
  that looks like a fix.
- **The sleeps had the same wiring hazard as the reporters.** `orbistoun-libc`'s three were taught
  to advance the clock and `orbistoun-kernel`'s `usleep` was not, so a guest polling with a short
  sleep still varied. Found by measuring again rather than by reading.
- **The address-map gate caught the new base before any test did.** `CONTROL_BLOCK_BASE` was
  declared and undocumented, and `orbistoun-service`'s map test failed with the base spelled out.
  Exactly what D510 built it for.

## Next

- The command buffer's twenty bytes of command, which are now reachable in the run that printed
  the pointer for every part of the run before the first thread - and this title's buffer is
  allocated after it, so the address still alternates.
- Whether a guest thread's allocations can be made to repeat without a scheduler: the arena's
  bump counter is shared, and a per-thread arena would make each thread's own sequence stable.
  Not attempted, and it is a change to how memory is placed rather than to how it is reported.
