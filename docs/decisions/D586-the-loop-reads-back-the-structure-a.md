# D586 - The loop reads back the structure a call was handed

**Status:** measured
**Date:** 2026-09-08

## The join that was missing

An argument dump shows thirty-two bytes at a pointer, which is its fixed window. The structure a
call was given is longer, and the interesting fields are past it: PPSA03416 hands
`sceKernelAprSubmitCommandBufferAndGetResult` a command buffer whose header fits and whose
command data is behind a pointer at `+0x10`.

Reading further meant typing an address out of a previous run's output into `ORBISTOUN_WATCH` by
hand - and that address is allocated while the guest runs, so until the snapshot stopped killing
the run on an address absent at entry (D580), it could not be asked at all. Every piece existed
and nothing joined them.

`Step::ReadStructure` is the join. Both ends are mechanical: the address comes from the finding's
own evidence, and what comes back is bytes.

```text
read libkernel::sceKernelAprSubmitCommandBufferAndGetResult's structure at 0x7400008a3520
read libSceAppContent::sceAppContentInitialize's structure at 0x400001a9c050
read libSceMouse::sceMouseInit's structure at 0x6000007fc910
read libkernel::sceKernelAprResolveFilepathsToIdsAndFileSizes's structure at 0x6000007fb458
```

**The first is the address that was typed in by hand the day before.** Nobody chose it.

## It observes, which is what lets it run on everything

Nothing is planted and nothing forced, so a verdict beside it needs no caveat. That is what makes
it safe to attach to *every* unimplemented finding rather than only to a wall somebody is already
suspicious of - an intervening step run that widely would poison every reading in the turn.

Reading what a call was handed is **not** implementing it. Writing the function stays a person's,
and it is a person's better informed.

## Three things had to be fixed to make it true

**The finding carried no arguments.** `Gap::Unimplemented` evidence was one sentence, and the
shim rendered the pointers beside it at print time - so anything reading a finding
programmatically could not see the call had been given a structure at all. Principle 13: a shim
holding what the crate should is how the other two drift. The dumps are in the finding now, and
only the ones the run could read - a scalar has no bytes, and rendering a count as an address
sends a dispatcher to whatever lives at that number.

**The turn discarded the worker's error stream.** `command.output()` captures and drops it, so
every byte a watch or a snapshot printed went nowhere - while `Taken::Watched`'s own
documentation said *"what it saw is on the run's error stream, by design"*. It was on the
child's, and nothing forwarded it. Now inherited, and gated on the axis rather than always: a
sweep is hundreds of boots and inheriting all of them would bury the result.

**The snapshot ran before the readable spans were published.** It sat in `prepare_diagnostics`,
during loading; the spans are installed in `arm_diagnostics`, just before entry. So a watch on an
*image* address - mapped since the module was placed - reported "did not exist when the guest
started". Moved after them, and an image address now reports a real diff.

Each of the three was a message asserting something the code did not do, which is the failure
principle 3 names, three times in one step's path.

## What this does not establish

**That the bytes are what the call saw.** The snapshot is read when the guest stops, not when the
call was made, so a structure the guest reuses or frees reports its final state. The command
buffer read this way already differs from the one the argument dump caught mid-run, and that
difference is real rather than a bug - it is two questions, and only the second is answered here.

**Nor that the first pointer is the right one.** A call's subject structure is conventionally its
first pointer argument, and picking between several would be a judgement this has no basis for.
A call whose interesting structure is its third gets its first read instead.

**Nor how much to read.** `0x100` is eight times the dump's window and enough for the live case.
It is a constant because a length that depended on what was found would make two runs read
different amounts and stop being comparable.
