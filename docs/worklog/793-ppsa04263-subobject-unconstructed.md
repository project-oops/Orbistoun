# 793. PPSA04263's null vtable is a sub-object, not the manager: `array[0]` is constructed and zeroes its own fields, but the object it later points to at `+0x2d0` is allocated-and-zeroed with no constructor run; the construction site evades a write-watchpoint

**2026-09-22** — worklog 792 read PPSA04263's wall as a virtual call through a null vtable and named the
next step: find where the `+0x2d0` object is constructed. This tick located that field's owner and one
level of its lifecycle, and confirms the wall is a sub-object whose constructor does not run - the
un-run-construction pattern, now pinned to the exact object.

## The manager is constructed; the sub-object is not

A write-watchpoint on the object-pointer slot `image+0x5521ef0` (`array[0] + 0x2d0`) fired at
`image+0x1927339`, `mov qword [rbx+0x2d0], 0` - part of a block (`0x19272e2..0x192734d`) that zeroes a
long list of the `0x4d0`-byte element's own fields (`+0x2d0`, `+0x2d8`, `+0x308`, `+0x32c`, `+0x338`,
`+0x358`, `+0x368`, `+0x38c`, `+0x394 = 0xffff`, ...). So `array[0]` - the **manager** - is constructed;
its constructor runs and initialises it, setting the sub-object pointer at `+0x2d0` to null.

By the fault, `[array[0]+0x2d0]` holds `image+0x5b37e98` (deterministic across runs), and that object is
**entirely zero** - including its vtable at offset 0, which is why `mov rax,[rdi]; call [rax+0x58]`
dereferences null. So the sub-object is **allocated** (its pointer is stored back into the manager) but
its **constructor never runs** (vtable unwritten). The manager's init is fine; the sub-object's is
missing.

## The construction site is not reachable by the watchpoint

The store that changes `+0x2d0` from `0` to `image+0x5b37e98` never fires the write-watchpoint - only
the constructor's zeroing does - so the allocation/store happens through a path the fixed-address
write-trap does not see (an aliased mapping or a vector store the byte-range check misses, the same class
of watchpoint blind spot D-noted before). Statically, that leaves the sub-object's `operator new` and the
constructor call that should follow it unlocated by the tools the loop has: the write-watchpoint misses
the store, and a lea/call scan cannot follow a `new`-then-construct sequence reached by computed
addressing.

This is the native un-run-construction wall at one more level of resolution: not "an object is
unconstructed" but "the manager builds, allocates its sub-object, and the sub-object's constructor is the
one that does not run" - and finding why needs the execution trace from entry worklogs 783/784/792 named,
which shows the alloc-then-construct sequence and where orbistoun's run diverges from completing it.

## Where this leaves the loop

Eight ticks past worklog 787's region-return win, the six retail titles are surveyed end to end and the
walls are uniform: three native/Il2Cpp titles at un-run construction reached by computed invocation, two
Unity-AGC titles at workload-array state (obSCEne REQ-...7a5d), one AGC title at the hardware-faithful
mapper. Each needs a capability the single-tick loop does not have - an execution/internal-call tracer, a
reference diff, or a pending hardware measurement. The productive next move is to build one of those
enabling tools (as 787's region-return was a small build that moved a wall), or to land honest work the
loop *can* do - real-export stubs from published semantics (PPSA04263 alone has `scePthreadGetaffinity`,
`sceKernelGetdirentries` and three others unimplemented) - rather than re-confirm the same tool-bound
walls.

## Gate state

No code changed - a decode that pins PPSA04263's null vtable to an allocated-but-unconstructed sub-object
at `array[0]+0x2d0` (manager constructed, sub-object not), and shows the construction site evades the
write-watchpoint. The `[experiment]` scratch the watchpoint runs wrote was restored. `./bin/orbistoun
check` green, worklog index regenerated, identity scan clean. No commit.
