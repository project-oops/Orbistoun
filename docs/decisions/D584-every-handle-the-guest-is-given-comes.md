# D584 - Every handle the guest is given comes from one region

**Status:** measured
**Date:** 2026-09-08

## D582 fixed the site where the bug was noticed, not where it lives

A thread handle was a host heap address, so `scePthreadSelf` answered something different every
run. That was fixed by serving control blocks from a fixed region.

**There were eight such sites.** `Box::leak` also produced thread attribute sets, mutex and
condition attribute sets, semaphore handles, synchronisation objects and the system version
block - every one of them an address the guest is handed and keeps, and every one different on
every run. Fixing the first and stopping is the same shape as a reporter wired into one of the
ways a run can end: the fix went where the symptom was.

So `blocks::block(words)` is the one place a guest-visible block comes from, and
`GUEST_BLOCK_BASE` replaces the thread-only region a day old. Block *n* is the same address in
every run.

## What a handle has to be, which none of this changes

D151 established it with a measurement rather than an argument: an opaque integer handle
reproduced a fault at a low address, because the guest reads a field through what it is given.
So a handle is the address of a real, aligned, zeroed block this crate owns.

**Nothing in that requires the host allocator to choose where.** Zeroed still matters and is
tested - a block carrying debris would hand the guest a pointer to somewhere arbitrary, which
faults far from the cause. Never freed still matters: a guest keeping a handle past its object's
life reads zeroes rather than whatever was put there next, which is a wrong answer that looks
wrong instead of one that looks right.

Sixty-four mebibytes, sized against a title that creates handles in a loop rather than the
handful a boot has needed. **Exhaustion falls back to the host heap rather than failing**, and
the fallback is not silent: `blocks::repeat` says which happened, because a run that took it has
non-repeating handles again and a reader comparing two runs cannot tell from the addresses.

## Measured, and the first number was noise

Two runs of PPSA03416, comparing the mapping sequence (D581):

| clock | identical mappings |
|---|---|
| `host` | 1 of 87 |
| logical | **28 of 88** |

The first pair measured after this change gave 1 of 88 and read as a regression. It was one
noisy sample: the sequence diverges once guest threads exist (D582), and *how far* two particular
runs agree varies on its own. **A single pair detects non-determinism and does not measure it** -
which is worth stating because the wrong conclusion was drawn from one for several minutes.

Five runs give five distinct sequences, so the full sequence never repeats; the length of the
agreeing prefix is the statistic, and it needs more than two samples.

## What this does not establish

**That handles repeating makes the run repeat.** Guest threads are real host threads, so the
order two of them reach the shared bump counter is the host scheduler's. This removes a source
of variation ahead of the first thread; it cannot remove the one after it.

**Nor that clustering is harmless.** Handles are now consecutive addresses rather than scattered
heap ones, and a guest bucketing them by low bits gets a different distribution. The mapping
count roughly doubled across this change and the wall did not move, so something about the
guest's behaviour did change. What, exactly, is not established here.

**Nor that eight was all of them.** Eight is what `Box::leak` finds. A block handed to the guest
some other way would not appear in that search.
