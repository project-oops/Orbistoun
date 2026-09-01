# D082 - Declarations reach the guest, and the first function is implemented

**decided** · 2026-08-19

There was no path from a declared function to a running one. The registry held names and
arities; thunk generation ignored it entirely, so **every** import landed on a recording
stub. Implementing a function would have changed nothing, and nothing would have said so.

**Implementations are bound to stub indices at load time.** A handler table, indexed by
dynamic symbol index, is consulted on the call path: present means call it, absent means
record and refuse. One dispatch path rather than two kinds of stub, which keeps the call
trace complete **as functions get implemented** - losing visibility of a call the moment
it starts working would hide exactly the traffic worth understanding.

`GuestFn` lives in `orbistoun-core`, the crate with no dependencies, because the
subsystems that write these and the thunk layer that calls them must agree on the shape
and neither should depend on the other to do it.

**`ServiceConfig::default()` is now the *working* configuration.** It defaulted to an
empty hash suffix, so the worker's registry hashed to values no module imports by and
every lookup silently missed. Everything built, everything ran, nothing resolved. A
default that cannot resolve a single import is a trap, and this one had been sitting
there since the suffix became available.

### The first implementation, chosen by measurement

`sceKernelDirectMemoryQuery` - 99.9% of every call four commercial executables make.
Implemented against a real direct-memory model (a sorted region list with allocate,
release, and merge-on-free) rather than a fabricated answer.

**The guest confirmed its own ABI.** Logging the arguments it passes gave the structure
size directly: `rcx = 0x18`, twenty-four bytes, matching the layout guessed from the
model. No reference consulted - the guest was asked.

**And it confirmed the walk works.** Changing the reported memory size from 8 GiB to
6 GiB moved the guest's second query from `0x200000000` to `0x180000000`, exactly
tracking. The guest reads the answer and advances correctly; this is not a stub being
tolerated, it is a real enumeration being driven.

### Where it stops, precisely

The guest walks to the end of memory, is told "no more regions", does not accept that
answer, and repeats the same query forever. The walk works; the **termination** does not.

The missing thing is the error code that means end-of-list. `GuestError::InvalidArgument`
is a placeholder deliberately chosen not to collide with real firmware values
(principle 3), so the guest cannot recognise it. That is now the next question, and it is
a good one: cheap to test, with a one-bit oracle - a candidate either stops the loop or
does not.

Worth stating plainly: the guest is no longer stuck because nothing is implemented. It is
stuck on one specific unknown constant, which is a much better problem to have.

