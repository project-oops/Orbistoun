# Threads are real, and the entry point was reading garbage


Guest threading, built as real host threads throughout - principle 6, and there was never
a version of this worth compromising on. `scePthreadCreate`, `Join`, `Self`,
`MutexInit`, `Lock` and `Unlock` all resolve to implementations now rather than to stubs.

Each guest thread gets a host thread, a stack of its own at its own address, and its own
entry into guest code with the argument the guest supplied. Stacks are spaced further
apart than a stack is tall, so an overrun hits a guard page rather than a neighbour -
packed adjacently it would read as memory corruption instead of as a stack overflow.

Locks could not be host `Mutex`es. That type's guard *is* the critical section, and the
guest locks in one call and unlocks in a different one with arbitrary guest code between
them; there is no host frame to hold a guard in. So it is a small state plus a condition
variable, with ownership tracked - a non-recursive lock taken twice by its owner is
refused rather than waited on, and releasing somebody else's lock is refused rather than
performed.

The affinity work is configuration rather than a decision (D200). A request is recorded on
the thread whatever happens to it, so a title that turns out to depend on placement can be
found rather than guessed at, and the mapping folds rather than clamps - clamping puts
back together threads the guest deliberately separated. The whole thing is
`thread::Settings`, serialisable, reachable from `ServiceConfig`, because a question that
needs a rebuild to try is a question nobody tries.

### Three surprises, in ascending order of how much they cost

**The knowledge file already had the answer.** Handles started as small opaque integers,
with a comment justifying it. The knowledge file - written a day earlier - recorded that
an unimplemented `scePthreadSelf` returned `0x7FFF0001` and a title faulted with
`read of 0x5`: that code being dereferenced at an offset. The guest reads fields out of
what this call returns, so a handle of `1` reproduces the same fault at a lower address.
Handles are now addresses of zeroed blocks (D151). The file is an *input*, not a report,
and it only got read because an unrelated test failed.

**A deadlock that passed every test.** The lock table was held while a lock was acquired,
so a waiter slept on the condition variable still holding the table, the owner could not
reach the table to release it, and nothing woke. Every single-threaded test passed. The
test that catches it blocks a real second host thread, which is the only shape that can.

**`rdi` was never empty (D152).** Adding an argument to `enter_guest` changed `rdi` from
an undefined clobber to an explicit zero, and two unrelated titles instantly faulted with
`read of 0x0` at the *identical* offset - `image+0x7a`. Identical across two titles means
the entry path, not title code. The guest entry point dereferences its first argument
register immediately, and for days it had been dereferencing whatever the compiler left
in that register: a stray host pointer, returning plausible garbage, good for another
sixty thousand bytes and thirty-seven imports of apparent progress.

There is a real process argument block now - a zeroed page, never written, because the
layout is not known from any lawful source. Both titles are back to thirty-seven imports
and `image+0xf2f6`. **Parity, not progress** - the same number, now meaning something.

Nobody would have written an experiment to check whether the entry point reads `rdi`.
Setting an inert thing to a defined value and watching two titles break identically
answered it in one run.

### State

Threading is built and tested but not yet *exercised by a guest*: both titles still die
before reaching a thread call. It is ready for whichever one gets there first, and for
obSCEne, which will call these deliberately rather than incidentally.

`orbistoun-translate`'s agreement test faults with an access violation. That is the GPU
thread's crate, it depends on nothing touched here, and the previous worklog entry already
names the suspect - a resource the Vulkan runner is not releasing between dispatches. Left
alone.

