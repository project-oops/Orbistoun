# D325 - The third place nobody wrote, and proving the poison fired


**decided** · 2026-08-27

`PPSA28061` reads `0x0` at `image+0x43c4` and dies. `docs/BACKLOG.md` had eliminated three
classes - not a missing import, not a wrong return value, not stale stack - leaving *"an
out-parameter nobody wrote, somewhere that is not the stack: a heap or direct-memory slot"*.

The stack has `ORBISTOUN_STACK_FILL`. The heap has `ORBISTOUN_HEAP_FILL`. **Direct memory
had nothing**, and it is the allocator this title actually uses:
`sceKernelAllocateMainDirectMemory` and `sceKernelMapNamedDirectMemory`, 18 calls each in
the first five hundred.

`ORBISTOUN_DIRECT_FILL` fills every fresh writable mapping. Result:

```
baseline    read of 0x0 at image+0x43c4, rdi=0x3, rax=0x0, 47 imports, 933 calls
d1 fill     read of 0x0 at image+0x43c4, rdi=0x3, rax=0x0, 47 imports, 933 calls
```

Byte-identical. **A fourth class is eliminated**: it is not reading unwritten direct memory
either.

### The part that took longer than the diagnostic

That result is worthless without one more thing, and it nearly went in the log without it.

**A poison that changed nothing and a poison that never executed produce identical output.**
Three runs across two titles all came back unchanged, and every one of them was equally
consistent with the fill never having fired - a `direct_fill()` returning `None`, a
protection check rejecting everything, a mapping path that was not the one the title uses.
Recording an elimination on that evidence is recording a class as tested when it was not,
which is worse than leaving it open, because it stops anybody looking.

So the fill counts what it does and the run says so:

```
orbistoun: direct-memory fill: 17 mapping(s), 71368704 bytes
```

Seventeen mappings and sixty-eight megabytes, all of it before the fault at call 933. *Now*
the elimination stands. A run that asks for a fill and reports none says so in those words -
**"asked for and never fired - nothing was tested"** - because that sentence is the one a
reader must never have to infer.

This is principle 3's third rule: *an intervention that moves a wall is not a diagnosis*.
The inverse needs stating too - **an intervention that moves nothing is not an elimination
until it has been shown to have intervened at all.**

### All three, and the class is closed

The same counting went into `ORBISTOUN_HEAP_FILL`, which had the identical problem: the
backlog's *third* elimination rested on an unchanged run with nothing showing the poison had
fired. It had - **5 allocation(s), 20320 bytes** - so that entry stands, but it stood on
luck until now.

With stack, heap and direct memory poisoned together, all three demonstrably firing:

```
read of 0x0 at image+0x43c4, rdi=0x3, rax=0x0, 47 imports, 933 calls
```

Byte-identical to the baseline. **The whole "an out-parameter nobody wrote" class is gone**,
not narrowed - there is nowhere left in guest memory for it to hide.

What remains is the other half of the backlog's sentence, and the run report has been
naming it all along:

```
! libSceSysmodule::sceSysmoduleLoadModule was called 3 times and nothing implements it
```

**A module asked to load that did not.** Answering it `Ok` was already tried and changed
nothing, which is exactly right and exactly the point: the title does not need the call to
succeed, it needs the module to *be there* afterwards. That is the difference between a
return value and a side effect, and it is the whole of what is left.

### Shape

`fill_for` is a pure decision - byte and protection in, byte or nothing out - so the
writable-only rule is testable without reserving memory. Writing to a read-only mapping
would fault inside the emulator and read as the guest's fault, which is the failure mode a
diagnostic must not have.


