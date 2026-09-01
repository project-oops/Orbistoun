# The tail found the wall, and the wall moved once


Added the ordered call tail to the trace (D154). The ranked list has always answered "what
does this guest spend its time on", which is right for choosing what to implement and
useless at a wall - `sceKernelDirectMemoryQuery` at 99.9% of calls says nothing about a
guest that just wrote through null. The ordering was recorded all along, in a ring sized
8192 while no run has passed 400. It just never left the process.

It paid for itself on the first run:

```text
sceKernelAllocateMainDirectMemory(0x1fe0000)
printf(...)
memset(0x0) x3
```

A guest asking for memory, refused, printing an error, then clearing a buffer it never
got. Blocker identified, argument order established (the first argument is a length in
both observed calls), failure explained - from data already being collected.

Implementing it: **37 -> 38 imports, 367 -> 372 calls, and the `memset(0x0)` calls gone.**
A real FURTHER.

Then the tail showed the next wall three calls deep, ending on an unnamed hash. Proposing
names and letting the hash confirm - nothing consulted - matched
`sceKernelMapNamedDirectMemory` and `sceKernelMprotect` (D155). The first argument agreeing
independently, a guest stack address where a caller wants its answer, is what makes that
more than a lucky collision. `Main` and `Named` went into the vocabulary so the repository
derives both names itself; a name confirmed in a session and not written into the grammar
is an assertion again.

### What did not land, and why it is parked rather than shipped

`sceKernelMapNamedDirectMemory` is written, tested, and **not registered**. Enabling it
takes PPSA28061 from 38 imports to 15 and moves the fault out of the guest image into host
code. Unregistering it restores 38 exactly, so the cause is certainly in that function or
in what it does to the guest's subsequent path - but *where* is not established.

It resisted diagnosis in a way worth recording. The last recorded call is the mapping call,
so the crash is inside it - yet a print at the top of the function never appeared, and one
inside the dispatcher never appeared either, with the string confirmed present in the
built binary. Whatever is happening is not the straightforward "my function faulted", and
guessing further would have been guessing.

Parking it is the honest call. A half-working mapping that regresses the only measure this
project has is worth less than no mapping, and the code and its tests keep the work while
the knowledge file records exactly why it is switched off (D157).

### One rule that came out of it

**Nothing reachable from a guest call may panic** (D156). Those frames are entered across
a `sysv64` boundary and unwinding through one is undefined - it does not present as a
panic message, it presents as an unattributable host fault. `next_multiple_of` is the
specific trap: it panics, it is the natural thing to write when rounding a length to an
alignment, and a guest may pass any value at all, including the all-ones word some callers
use for "no preference". Every guest-reachable rounding is checked now, with hostile-value
tests asserting refusal rather than a crash.

That rule was found while chasing the regression and is worth more than the regression is:
it applies to every implementation this project will ever add, and an index, an unwrap or a
division would fail exactly as illegibly.

