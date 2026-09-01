# D344 - Guest threads are asked to stop, not made to


**decided** · 2026-08-27 · a modelled state made real, and the obvious way to do it is a trap

`Execution::Suspended` was derived, documented and inert: backgrounding a title reported it
while every guest thread ran on. A value that describes a behaviour without causing one is
the failure this project keeps finding in its own work, and it had been sitting in the
session model for a day.

**The obvious implementation is dangerous.** Suspending a thread at an arbitrary instruction
can catch it holding the host C runtime's heap lock, and then the next allocation anywhere in
the worker blocks forever - including on the thread that would have issued the resume. That
is not a stopped worker, it is an unrecoverable one, and it happens only sometimes.

So threads park cooperatively. Every guest call passes through **one trampoline**, which is
the natural place: a thread stopped there is in our code, holds no guest lock, and is about
to do nothing that cannot wait. The check is a load and a branch on the not-parking path,
allocates nothing and takes no lock, which is what principle 9 requires of anything on that
path anyway.

**The cost is stated rather than discovered.** A thread that stops calling imports never
parks - a compute loop, a spin on a flag, a wait for something that will never arrive. Same
shape as the run limit, which exists because *"a guest with every import unimplemented can
settle into a loop waiting for something that will never happen"*.

The difference is that this one is **counted**. The run report says *"backgrounded: 2 of 3
live guest thread(s) parked"* rather than saying "suspended" and letting a reader assume all
of them did. A mechanism with a known hole is fine; one whose hole is invisible is not.

Two details that are the whole safety of it: the park flag is process-wide, so ending a title
releases it - otherwise the next run's threads park at their first call, hanging for a reason
belonging to the run before. And the two tests for it had to be **one** test, because
process-wide statics plus a parallel harness is a race that reads as the mechanism being
broken rather than the tests being wrong. That is the second time today.

