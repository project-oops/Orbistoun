# Every diagnostic axis, and a dispatcher that turns the loop


Ten diagnostics are listed by `orbistoun-cli env`. One of them had ever been swept
automatically. `orbistoun-propose::axis` makes the rest sweepable on the same terms -
`Fill` for each region, `Map` around the faulting address - and `GuestTrial::probe` runs a
list of them against one baseline, clearing **every** diagnostic variable before each run so
one experiment cannot inherit another's or the shell's.

Against the live wall the answer is a clean negative: six axes, one second, nothing changed.
Not for want of asking - heap and bss fills and every reservation had simply never been run.

**The interesting part was the one that looked positive.** Poisoning zero-initialised
statics moved the fault to a completely different address. It took a second signal to see
that the guest had reached 8 distinct imports instead of 23 - the poison broke it long
before it got anywhere near the wall. `Change::BrokeEarlier` now holds both numbers and is
explicitly not notable, so it cannot sit beside a real lead. D129 recorded the same lesson
about the progress verdict; this is the second arrival at it (D232).

`orbistoun-propose::turn` is the other half. It reads the report's own `Gap` taxonomy and
maps each kind of wall to a fixed step, then runs the mechanical ones. Deliberately a
dispatcher and not a chooser, because a boot against a wall costs about 0.13 seconds and a
model answer costs 5-20 - so exhausting the space beats selecting from it, and a prior saves
nothing (D231). The model survives in exactly one branch, naming a bare hash, which is the
only place its output was ever measured to be non-redundant with the string harvester. A
test pins that, so widening it has to be deliberate.

One turn against a real title: **8 findings, 9 steps, everything mechanical done in 2.6
seconds**, stopping at the ones that are a person's - each with a sentence saying why.

**And it found a bug in itself on the first live run.** A fault's `subject` is the region
the guest died in, not the call that led there, so the first attempt swept `image` and
planted nothing at all. Visible only because `NeverPlanted` is a distinct outcome; without
it, six slots that changed nothing reads as a clean elimination. That is twice now that one
distinction has caught an experiment that never ran (D233).

### Every unimplemented call in PPSA02664 is eliminated

Seven return values dyed in one run (`7 answered`), fault byte-identical. Offset-zero
out-parameters for the other six swept in two more runs (`4 planted, 14 refused` and
`7 planted, 23 refused` - the refusals are non-pointer arguments, counted rather than
guessed). The wall function itself had already had all eight slots swept.

So no unimplemented import in that title supplies the missing base, by return value or by
out-parameter. `memalign` - implemented, called once, returns a pointer - is eliminated too,
now that a forced answer reaches implemented functions (D234).

**One diagnostic turned out to be the wrong tool.** `ORBISTOUN_BSS_FILL=a5` faults far
earlier, at `image+0x14c2c15` with `rax=0xa5a5a5a5a5a5a5a5`: it fills 1.25 MB of statics and
the guest trips on the first one it reads. It cannot isolate one static, which is what the
question needs, and `ORBISTOUN_POKE` already does that. Not a result - a coarse instrument
reported as coarse.

### The other two walls, with tools they predate

**PPSA28061** had never been seen with a full register set. `rbx` holds a **host heap
pointer** - one of our own allocator's results - so the guest is holding a valid allocation
when it dies; `r13=0` is the null it dereferences; and `rcx=rdx=0x7fff0001`, a sentinel that
also appears as an argument in PPSA02664. Three facts from one run of a wall that had been
cold for weeks, all of them already in the trace.

**PPSA04263** calls **four** imports, exactly one of which is unimplemented:
`libc::0x92f57c2dc704346f` - the same function PPSA02664 calls, the one handed the guest's
entry point. It asks `sceKernelGetDirectMemorySize` once and then walks the map 115 million
times.

That suggested the guest never accounts for all the memory it was told exists - which would
loop forever. **Disproved without spending a run:** the map covers `DIRECT_MEMORY_SIZE`
with no gaps under every shape, and a test asserts it. Cheaper to read the test than to
schedule an experiment.

### The structural hypothesis, and its measurement

With every call eliminated, the remaining explanation was that the base was never produced
by a call. The dynamic-table parser ignored `DT_INIT_ARRAY`, so the guest's global
constructors had never run - which would leave exactly the zeroed global the fault implies,
while still showing the guard traffic the trace has.

Parsed the tags and measured: **`init_array` is absent from all three titles at a wall.**
The hypothesis is dead, the parsing stays with tests, and the absence is now a fact rather
than an unexamined gap.

Worth the entry because the next step would have been an initialiser executor, which would
have run an empty list and been indistinguishable from a working one. One temporary print
avoided that.

Two loose ends kept rather than smoothed over: `DT_INIT` reads `0x10` on all three titles -
identical across unrelated games, and not a plausible code address when the entry point is
`0x70` - and PPSA28061 has a `preinit_array` pointer with size zero.

