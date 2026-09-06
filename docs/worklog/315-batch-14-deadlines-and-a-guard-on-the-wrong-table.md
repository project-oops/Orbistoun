# 2026-09-02 - (/loop) Bulk port batch 14: the timed acquisitions, and a guard that checked the wrong table

```
documented   715 needed, 239 missing   ->   715 needed, 225 missing
```

Fourteen names: seven functions and the `posix_`-prefixed twin of each. This was primitive
work before glue, as expected - and it turned up two things that were not on the list.

## The primitive work

`sync::cond_wait` already took a timeout; nothing else did. The mutex said "how patient are
you" by having a second entry point, the semaphore by having a third, the read-write lock by a
bare `bool`. A deadline would have been a **fourth spelling of one idea**, so it became one
instead:

```rust
pub enum Blocking { Never, Forever, Until(Instant) }
```

`lock`/`try_lock` collapsed into `acquire`, `semaphore_wait`/`semaphore_try_wait` into
`semaphore_wait`, and the two read-write acquisitions lost their `bool`. Every blocking loop in
the module now goes through one `wait_while` helper. All 33 existing tests passed unchanged
afterwards, which is what says the collapse preserved behaviour rather than merely compiling.

`TryLock` became `Acquisition`, because a type named after the impatient case was about to be
returned by a call that may block forever. `Busy` now means "did not get it" and the caller
knows *why* from the patience it asked for - which is why timing out is not a fourth variant.

**The deadline is re-read every turn**, and that is the whole of the difficulty. Handing the
full remaining span to each `wait_timeout` restarts the clock on every spurious wake, and this
module's releases notify *all* waiters, so a contended lock would be waited on forever by a
call that was asked to give up in a millisecond.

## The functions

`pthread_mutex_timedlock`, `pthread_rwlock_timedrdlock`, `pthread_rwlock_timedwrlock`,
`sem_timedwait`, `sem_getvalue` (POSIX.1-2008), and `sem_reltimedwait_np`,
`pthread_cond_reltimedwait_np` (Solaris `sem_timedwait(3C)` and `pthread_cond_timedwait(3C)`,
which document both spellings and their arities).

**The POSIX calls take an absolute deadline; the `_np` pair take a relative span.** Two
readers, not one with a flag, because neither misreading fails loudly: an absolute time read as
relative restarts the clock every retry and the loop never ends, and a relative span read as
absolute expires instantly, because one second is a moment in 1970.

## Two things that were not on the list

### A timeout had no error code, and the honest fix was a decision (D476)

`ETIMEDOUT` has never been seen from the target, and the `errno` module said in as many words
that everything in it had been. Answering the measured-but-wrong `EBUSY` would make a retry
loop spin forever; answering the placeholder would report "not handled" for a call that handled
it exactly. So the module now carries a **published** tier beside the measured one, with the
line between them stated - and `TIMED_OUT` names the one conformance check that would promote
it. Nothing was loosened to let it in.

### A guard was checking a different table from the one it named

`no_name_is_delegated_twice` said:

> A duplicate would mean the registry's last-wins rule picks one silently.

...and then checked the *delegation* table, where a duplicate is harmless because both rows are
identical. The list that rule actually applies to is the module's declarations, and **three
names were duplicated there**: `pthread_cond_timedwait` and `pthread_attr_setschedpolicy` each
had a live arity shadowed by a stale zero left behind in the not-served list, and `sched_yield`
was declared twice over.

So batch 13's `pthread_cond_timedwait`, whose comment says in capitals that it takes an
absolute deadline as its *third* argument, was registered as taking none. Nothing observable
broke - arity is metadata for the trace and the gap report, not for dispatch - which is exactly
why it survived. It is invisible until somebody reads a trace of a timed wait and finds it took
no arguments.

Fixed, and `no_name_is_declared_twice` now guards the right list. Both guards were **watched to
fail** before being trusted: a duplicate was reintroduced and each named its offender.

## A hanging test is not a failing test

The test that matters most here is the one for the deadline arithmetic: readers churn in and
out while a writer waits, so every release wakes it with nothing. The arithmetic was reverted
deliberately to check the test caught it.

**It did not fail. It hung** - which is the honest symptom of an unbounded wait, and useless as
a guard, because a hanging test stops the suite and reports nothing about what broke. Rewritten
to do the wait on its own thread behind a channel, so the deadline that decides the verdict is
the one on the `recv`. Reverted again: it now fails in five seconds naming the cause.

## The gates I had not been running

Every batch worklog since 309 ends "clippy clean, fmt clean, kernel/posix tests pass". All of
that was true. **It was also not the whole test suite**, and this tick finally ran
`cargo test --workspace` - which fails, and has been failing for some time.

The cross-crate guards live in `orbistoun-service`, because they are the only ones that can see
more than one subsystem at once. Running `-p orbistoun-kernel` never reaches them. So the
narrow command passed, the worklog reported what the narrow command said, and the reader was
left to assume it meant more than it did.

That is the same over-claim principle 3 keeps catching, one level up: **not a wrong measurement,
a true measurement reported as a broader one.** Worklogs 310-314 should be read as "the crate
tests passed", which is what was actually checked.

Four failures, from three causes. One was mine and is fixed; two are not fixed here, and each
says why.

### Fixed: `posix_pthread_barrier_init` was implemented but declared nowhere

Batch 11 split the POSIX-signature barrier init from the vendor one (D385) and wrote in the
exception list that it was "declared in the POSIX module". **It was not.** The POSIX module
declares `pthread_barrier_init`; the internal entry point it delegates to had no declaration at
all, so nothing reached the registry under that name.

Its own twin showed the fix: `posix_pthread_rwlock_init` is declared in `libkernel` alongside
the vendor spelling it splits from. The barrier init is now declared the same way, and the
exception - which was excusing something on a false premise - is gone.

### Not fixed: `sysctlbyname` is implemented twice, in two crates

`orbistoun-kernel` and `orbistoun-libc` each declare and implement it. A NID is the hash of a
name alone, so both resolve to one NID and the registry's last-wins rule picks one silently.

**This predates today** - the symbol appears on neither side of the day's diff, so both
implementations were there at the last commit, and the three guards that trip on it
(`no_symbol_is_declared_twice`, `nids_differ_per_symbol`, and the declared-count check) were
already red.

It is left for its own tick because **it is a merge, not a deletion.** The first look said the
`libc` one was a strict superset - it answers three measured integer knobs (`hw.ncpu`,
`hw.pagesize`, `machdep.tsc_freq`, all read off a console), plus `kern.ostype`, plus it reports
unknown names once so they become a work list. That was wrong on one point: the two differ on
an unset `kern.osrelease`, where `libc` refuses and the kernel answers an empty NUL-terminated
string - "a knob that exists with no value, rather than an invented one", which is D447's
reasoning and is the behaviour an obSCEne conformance case checks. Deleting either one drops
something deliberate. Doing that carelessly at the end of a tick is how a hardware-verified
check quietly regresses.

### Not fixed: 112 implemented functions have no knowledge entry

`every_implemented_function_is_written_down` exists because "implementing something without
recording what was learned is how the knowledge ends up existing only in a conversation". It is
red, and the count is the point: **112 functions, of which 84 were registered today** by the
bulk batches and 28 predate them. So this gate was red before this run started, and the bulk
port made it four times worse without noticing.

Not backfilled here on purpose. Each entry wants a real purpose, a real provenance and real
assumptions, and 112 formulaic rows written at speed would satisfy the guard while defeating
what it is for. The material exists - every one of these has a doc comment stating its
specification and a worklog entry - so the honest version is a pass that derives each entry
from what is already written, which is a unit of work rather than a footnote to this one.

**That is the next tick**, ahead of more porting: the gap number is worth less than the record
behind it, and continuing to add functions while the guard that records them is red only widens
the hole.

## State

clippy `--tests` clean, fmt clean, identity scan clean, nothing committed.

Kernel and posix crate tests pass (39 in the sync suite, six of them new). **`cargo test
--workspace` does not** - four failures, all described above, one of them fixed since and three
remaining across two causes that both predate this batch.

**Next**: 225, but not by porting. The knowledge backfill and the `sysctlbyname` merge come
first, because both are guards this project already wrote and is currently ignoring. After
them: the socket scatter/gather (`sendmsg`, `recvmsg`, `sendto`, `recvfrom`) can reuse the
`iovec` reader from `readv`/`writev`, though `msghdr` is a different structure and its layout
has to be read rather than assumed. Thread cancellation still needs a design rather than a spec
reading - storing the state without ever delivering a cancellation would be a lie, and saying so
is the minimum if only the storage lands.
