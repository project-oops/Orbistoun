# 471. The report can see a guest sitting still

**2026-09-09** - directed, continuing 470

470 recorded that PPSA25872 stops after installing a signal handler and raising signal 30, and that
establishing "blocked rather than slow" took two runs at different limits and a hand comparison.
This pass made the report say it from one run.

## The sampler

The thread enforcing the wall-clock limit slept for the whole duration and then collected, so the
most it could ever know was that the duration had passed. It now wakes every 250ms and reads two
counters that already existed - import calls, and system calls through a new one-load accessor
beside the allocating `syscalls_in_order` - and records when their sum last moved. `CallTrace`
carries a `Quiet` block; the run report and the worker summary both print it (D646).

First run:

```text
  standing 310979 of 310987 calls answered by an implementation (8 on stubs, 0%)
  quiet    it made no call in its last 19.7s of 20.0s - the last import or system call was at 0.2s
           ! it stopped asking the host for anything well before the clock ran out - either it is
             waiting on something that has not arrived, or it is working inside its own code
           try ORBISTOUN_LIMIT=80 - an identical call count there means it is blocked
```

**All 310,987 calls land in the first fifth of a second.** 470 showed the guest was not using its
extra time; this shows it never used any of it.

## Surprises

- **The measurement was stronger than the thing it was built to replace.** The two-run comparison
  said "no progress between 20s and 90s". This says the guest was finished at 200ms - a much
  sharper fact, from a cheaper observation, on every run rather than on the ones somebody thought
  to check twice.
- **Writing the negative test caught the negative test.** The "brief silence in a brief run" case
  was fixtured as 900ms of a 2,000ms run, which is not half, so it would have passed for the wrong
  reason. The failing assertion was my own arithmetic (D646).
- **The verdict deliberately stayed weaker than the temptation.** Silence is not proof of blocking;
  a guest computing in its own code looks identical. The line names both readings and names what
  separates them.

## The request mesh

A cross-project request mesh is now defined at `<mesh>/README.md` - every OOPS project owns an
inbox, files into others', and resolves its own. Orbistoun's is `<mesh>/Orbistoun/worklog.md`.
obSCEne's existing bus is one node of it rather than the only channel.

This pass: serviced the first request to arrive (oops-sdk asking for a tile-swizzle cross-check,
resolved `not-possible` - there is no tiler in this repository, and transcribing the requester's
own equations to produce agreeing numbers would be confirmation by construction); filed three
requests - the signal-delivery contract to obSCEne, three unnameable import hashes to SELFish, and
to Prosperous the question underneath obSCEne's `not-possible` on `libSceAgc`: what makes the
runtime linker treat a registered homebrew title as a compatibility host.

## Next

- The three filed requests, and whatever they answer.
- The AGC contract - now being approached from the transport side rather than the probe side.
- The libc data-object request, still open, still PPSA21564's wall.
