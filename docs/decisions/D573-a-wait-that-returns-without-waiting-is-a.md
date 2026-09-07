# D573 - A wait that returns without waiting is a busy loop, so the wait now waits

**Status:** measured
**Date:** 2026-09-07

## What the guest was doing

With the ring circular (D571) and all six arguments recorded (D570), PPSA25872's last 48 calls
were 48 calls to the futex wait, and the printer summarised them on one line as
`(0x7400007549e8) -> 0x7fff0001 x48`. Read from the JSON rather than the line:

- **Thirteen distinct first arguments**, cycling, every one `0x150` bytes from the next - one
  array of thirteen objects, each with a word at the same offset.
- Every call `(address, 0, 0, 0, 0, leftover)`: wait while the word holds zero.
- One call site, `image+0x1ae4c75`. `scePthreadCreate` was called **thirteen times**.
- `sceKernelSyncOnAddressWake` was called **thirteen times**, on addresses `0x30` below the
  waited words, with `(address, 1, 1, 1, 0x7ffffffe, 0)` in both calls whose arguments were
  captured.

So D566's "one address, eleven million times" was one thread's worth of a thirteen-thread spin,
read off a line that shows a run's first call and nothing about the other forty-seven (D574).

## The model

FreeBSD's `_umtx_op(2)` is the lawful reference and obSCEne's own note names it as the mechanism:
`UMTX_OP_WAIT` compares a word with a value and sleeps only while they are equal, returning
success at once when they differ; `UMTX_OP_WAKE` ends up to *n* of those sleeps and is not
remembered when nobody is asleep. The word carries the state; the wake only ends a sleep.

`sync::wait_on_address` and `sync::wake_on_address` in `orbistoun-kernel` are that, with the
three things that make a futex correct rather than merely usual:

- **The compare happens under the queue's lock.** A waker writes the word and then wakes. A
  sleeper that read the old value has joined the queue before the lock is released into the
  sleep, so the wake finds it. Read the word first and take the lock second, and a wake in the
  gap is lost forever. That is why the read is a closure handed in, not a value looked up first.
- **A wake with nobody asleep leaves nothing behind.** Tokens never exceed sleepers. A token kept
  from an early wake would end a later, unrelated sleep for no reason.
- **Exactly the count asked for wake.** PPSA25872 asks for one per thread. Waking everybody on a
  count of one is a spurious wakeup a pool handing out work one wake at a time would turn into
  two workers on one job.

Eight tests pin the contract, and the negative one - a wake before the wait is not remembered -
was watched failing with the guard removed, per principle 3's rule that a guard nobody has seen
reject something is a guard nobody knows anything about.

## Three choices, and what each rests on

**Sixty-four bits.** In the 12.40 layout the wait's entry point is the one `Wait64` uses while
`Wait32` has its own (D572). A guest whose word is 32 bits beside a non-zero neighbour would never
block here; nothing observed does that.

**Arity two.** Across the 48 calls the ring held and the 90 it dumped, registers three to five
read zero on every wait. **A non-zero third register is refused with the placeholder rather than
waited on.** It may be a timeout in a unit nothing here has established, and modelling it as
"forever" would be a plausible answer to a question that was not read. The placeholder is loud
in a trace and names the call.

**Forever.** With zeros everywhere a timeout could be, the observed calls wait indefinitely. The
`TimedOut` arm exists so the match stays total, answers the published `ETIMEDOUT` under the
measured vendor base, and is unreachable today.

## Measured

Same title, same twelve-second limit, same build but for the two functions:

| | placeholder | modelled |
|---|--:|--:|
| calls | 2,580,950 | **307,226** |
| on stubs | 88% | **0%** |
| `sceKernelSyncOnAddressWait` | 2,273,750 | **26** |
| `sceKernelSyncOnAddressWake` | 13 | 13 |
| imports | 121 | 121 |
| fault | modules+0x117e1b | modules+0x117e1b |

The spin is gone. **The wall is not moved**, and the verdict says `same` - which is the honest
answer, because the wall was never the wait. The main thread dies in a path walk under
`/app0/Media` reading above the top of its stack, and before today that fault sat behind two
million calls of noise. The tail now reads `pthread_equal`, `pthread_self`, `setjmp`, `memset`:
the calls actually before the fault.

Twenty-six waits and thirteen wakes is consistent with each thread waiting once on the word the
main thread wakes and once, forever, on the word nothing ever sets. **That is an inference**: a
trace does not record which thread made a call, and both a wake and a mismatch answer zero.

## What this does not establish

**That the guest is now correct past the wait.** Thirteen threads sleeping forever on words
nothing sets is honest and may also be wrong - if the platform's wait had a timeout the guest
relies on, the second wait would return and the workers would run. The refused third register is
where that would show, and it has not.

**Nor what moved the wall.** The record of 2026-09-04 has this title at 129 imports and seven
million calls, halting at the time limit. Today's build reaches 121 and crashes, deterministically
across four runs, before and after this change. Two environmental differences were found and
one was tested: the guest's first argument carried a host path (a launcher fault, D574) and a
relative path reproduced the crash exactly; the cache directories were empty at session start
and were not tested. The cause is open.
