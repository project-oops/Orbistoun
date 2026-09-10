# D646 - The clock samples instead of sleeping

**Status:** measured
**Date:** 2026-09-09

## "Ran to the time limit" is two different runs wearing one sentence

D645 established that PPSA25872 is blocked rather than slow, and the only way to establish it was
running the title twice at different limits and comparing the totals by hand. Nothing in the report
said it, and nothing in the report *could* say it: the thread enforcing the limit slept for the
whole duration and then collected, so the most it ever knew was that the duration had passed.

That is the same fault as D177, one field along. A guest that calls `abort` was once described as
having run to the time limit, which is the opposite of what happened; a guest that stops calling
anything after two hundred milliseconds is described the same way, and it is equally the opposite.
Both are a third outcome with no field to live in.

## One number, sampled, costing nothing

The limit thread now wakes every 250ms and reads two counters that already exist - import calls
(`total_calls`) and system calls (a new `syscalls_made`, one relaxed load rather than the
allocating walk `syscalls_in_order` does). It records when that sum last moved. The trace carries
`Quiet { silent_ms, last_activity_ms, run_ms, sample_ms }`, and every report that prints it says
what was counted.

Both counters, because either alone lies about a whole class of guest: a commercial title calls
imports and almost no syscalls, and an open-toolchain payload builds a gadget and then calls
nothing but syscalls. Watching one would report the other as permanently silent.

`sample_ms` is a field rather than a constant in the printer because **it bounds the claim**. A
silence measured at quarter-second resolution is not known to the millisecond, and a line printing
one without the other reports more than its measurement supports.

## What it said the first time it ran

```text
  standing 310979 of 310987 calls answered by an implementation (8 on stubs, 0%)
  quiet    it made no call in its last 19.7s of 20.0s - the last import or system call was at 0.2s
```

**All 310,987 calls happen in the first fifth of a second.** The two-run comparison in D645 showed
the guest was not using its extra time; this shows it was never using any of it. Nineteen point
seven of twenty seconds is a guest sitting still, and that is now on the report of every run that
hits the clock, from one run, without anybody knowing to look.

## What it deliberately does not say

It does not say "blocked". No call crossing into the host is the measurement; a guest waiting on
something that has not arrived and a guest computing inside its own code produce the same silence,
and this branch cannot separate them. The report names both readings and then names the thing that
*would* separate them - a longer limit with an identical call count - rather than picking one. A
verdict naming a cause must come from the branch that determined it, and this branch did not.

For the same reason it is `None` rather than zero on a run that faulted, stopped itself, or spent
its call budget: in all three the guest was going when the run ended, so there is no silence, and a
`0.0s` would be a measurement nobody made.

## The guard was made to fail

Three tests, and the two that matter are the negative ones: a run still calling at the end is not
notable, and a silence under a second is not notable however large a share of the run it is. The
floor exists because a short run is mostly startup and sample interval. Writing the second test
caught the fixture's own arithmetic being wrong, which is the argument for writing it.
