# D476 - The `errno` table carries published values too, in a tier that says so

**assumed** - 2026-09-02 (user-directed bulk port, batch 14)

The timed acquisitions - `pthread_mutex_timedlock`, `sem_timedwait`,
`pthread_rwlock_timedrdlock` and the rest - all have to report one condition nothing else in
this project had ever reported: a wait that ran out of time. There was no value to report it
with, and the reason is a rule worth keeping.

`orbistoun-core`'s `errno` module said of itself:

> Only values somebody has actually observed coming back are listed - this is a record of
> measurements, not a copy of `errno.h`, and a name here is a claim that the target produced
> it.

That is a real, checkable property and every one of its nine entries earned it: seven came out
of a single conformance run on a console, each provoked deliberately. `ETIMEDOUT` has not been
provoked on hardware by anybody, because until this batch nothing here could ask for a bounded
wait.

## The three options, and why the third

**Answer `EBUSY`, which is measured.** Rejected: it is a *wrong* answer, which principle 3
forbids more firmly than an unverified one. POSIX has `pthread_mutex_timedlock` answer
`ETIMEDOUT` and never `EBUSY`, and the difference is one a guest acts on - the ordinary retry
loop treats busy as "go round again" and timed-out as "give up". A guest handed the wrong one
of those spins forever, and nothing in a trace would say why.

**Answer the project's own `Unimplemented` placeholder.** Rejected for the opposite reason:
the call *is* implemented. The wait ran, the deadline passed, and the outcome is known exactly.
Reporting "not handled yet" would be as untrue as reporting the wrong errno, in the other
direction, and it would mislead the gap report as well as the guest.

**Add the value, and say how it is known.** Taken. `ETIMEDOUT` is 60 in the documented `errno`
numbering of the platform's FreeBSD ancestor - a citable source, which is exactly what
principle 1 calls `published` rather than `measured`. The project already has the vocabulary
for this distinction and uses it everywhere else; the errno module was the one place that had
collapsed it, by holding a single tier and describing the whole module in the terms of that
tier.

So the module now has two groups with a line between them, and its own note explains which is
which. **The measured tier keeps its exact claim** - nothing was loosened to let the new value
in, which is the part that matters. A blanket sentence that had become false is now two precise
sentences that are both true.

## What is actually uncertain here is small

The encoding around the number was measured, and measured well: `0x8002_0000 | errno` was
confirmed across seven values from five unrelated call families in one run on a console. So the
unverified part of `TIMED_OUT` is the number 60 alone, taken from the documented ancestor of the
platform's own kernel.

That is a narrow gap and, unusually, a **nameable** one. The entry carries the probe that
settles it: take a lock, call `pthread_mutex_timedlock` on it with a deadline a millisecond
out, and record what comes back. One conformance check promotes it from published to measured,
and the promotion is a one-line change.

This is why the tier is worth having rather than being a place to hide guesses. An entry that
cannot name what would confirm it does not belong in either group.

## Status

`assumed`, because the value has not been seen from the target. It becomes `measured` the day
somebody runs the probe named above - and until then, a title behaving oddly around a timed
wait is the first place to look.
