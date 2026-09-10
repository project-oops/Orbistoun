# D629 - A module handle now narrows the answer, for one handle

**Status:** measured
**Date:** 2026-09-08

## The loudest defect on the differential

`900-surface/control` is obSCEne saying *a symbol that does not exist reported present; every count
in this section is meaningless* - a defect that reports its own blast radius over ninety-nine
census entries (D622). Behind it is a divergence already on record: `sceKernelDlsym` looks a name
up in one flat table and **never consults the module handle it was given**, so libkernel answers
for `memcpy`, which the console refuses with `0x8002_0003`.

The record said the fix needed *"a per-module export list for the platform's own libraries"*, which
this project does not have and must not invent. That was half right. It does not know what the
console's libkernel exports; it does know what **it itself declares** in libkernel, and that is
enough to correct a wrong success without inventing a refusal.

## Narrowed only downward, and only where it can be

Three conditions, all required:

- the handle is `0x2001`, libkernel's, the one value three measurements agree on;
- the name resolved **from the stub table**, so this project declares it somewhere;
- and that somewhere is not the libkernel family.

Everything else is untouched. A name nothing has a library for still resolves - *"nobody said"* must
never become *"this module does not export it"*. A name only the **guest's own binary** exports
still resolves, because PPSA02664 asks for its own `scriptingGetMem` through this call (D517) and
breaking a working path to fix a different one is not a fix. Every other handle still resolves
anything.

So the change can turn a wrong success into the measured refusal, and cannot turn a working
resolution into a failure on a guess.

**The whole `libkernel` family counts.** `libkernel_fs` and `libkernel_sync_on_address` are this
project's own subdivisions of one console library - `sceKernelWrite` is in the second and the
console exports it from the first - so treating them as separate modules would invent a divergence
rather than repair one.

## Published by the service, because no one crate knows

The three declarations live in three crates that must not reach sideways for each other (D536), and
`orbistoun-kernel` cannot ask `orbistoun-service` for anything. So the service publishes the set
into `orbistoun-thunk` beside the by-name stubs it already publishes there, and the kernel reads it.
`name_is_libkernel` returns `Option<bool>`: `None` and `Some(true)` are different states and both
mean *do not refuse*, because a list nobody installed is silence.

Built once, because `sceKernelDlsym` is the busiest call in the corpus - three hundred thousand in
one payload run - and consulting a knowledge base per call would be a sink that changes the program
it observes (principle 9).

## What it does not do, said plainly

**`110-modules/symbol` is unchanged by this.** That check does not use handle `0x2001`: it walks a
list of module paths calling `sceKernelLoadStartModule` and asks whichever handle first succeeds,
which under orbistoun is an `/app0` handle counting from `0x40` - every firmware path is refused
with `ENOENT`. So the measurement that motivated this still reports `0x0`.

The narrowing is right, and it is right for a handle that check does not exercise. Making it cover
an `/app0` handle would mean deciding what a *title's own module* exports, which orbistoun knows
only for the executable, and would risk refusing the resolution path the payload makes three
hundred thousand calls through. Not attempted on that evidence.

`dlsym_divergence.rs` is rewritten to what now holds, and it asserts three states rather than one:
inert before a list is published, the console's `0x8002_0003` with nothing written after it, and a
libkernel-declared name still passing through. The third exists because a guard that refused
everything would satisfy the second.
