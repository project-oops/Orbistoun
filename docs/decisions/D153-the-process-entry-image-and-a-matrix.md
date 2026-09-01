# D153 - The process entry image, and a matrix that ruled out three hypotheses at once


**decided** · 2026-08-20

D152 established that the entry point dereferences its first argument register and
nothing more. This built the rest properly and then measured which parts of it matter.

**What was built.** `orbistoun-loader::process` lays out the System V AMD64 initial
process stack - the strings, the auxiliary vector, the environment pointers, the argument
pointers, the count - with `rsp` sixteen-byte aligned at the count. That layout and the
auxiliary vector types are **published standards**, and the target kernel is
FreeBSD-derived, so this is the documented convention rather than a guess. Principle 1's
best case, and rare here.

The auxiliary entries that describe *this run* - the entry point, the load base, the page
size - are derived from the loaded image rather than configured, because a setting able to
disagree with the image is a setting able to lie. `AT_PHDR`, `AT_PHENT` and `AT_PHNUM` are
deliberately **absent**: a runtime that walks its own program headers needs them, they are
derivable in principle, but the placed image does not record where the headers landed and
inventing an address would hand the guest a pointer into whatever is there. Absent is a
case a program can handle; wrong is not.

**What was not decided, and why.** Whether the vendor's entry point follows that
convention is not published. So both readings are settings:

- `Convention::Process` - jump, `rsp` at the argument count. A process cannot be `call`ed:
  the pushed return address lands exactly where the count must be, and leaves the stack
  eight past the alignment the standard requires. The two are not reconcilable by an
  offset.
- `Convention::Function` - call it, which is what this did before.
- `EntryArgument` - the image address, a zeroed block, or nothing.

All of it reads from the config file the `paths` command has always printed and nothing
has ever read until now.

**The measurement.** Six runs against PPSA28061:

| convention | argument | imports | fault |
|---|---|---|---|
| process | image-address | 37 | `image+0xf2f6` |
| process | zeroed-block | 37 | `image+0xf2f6` |
| process | zero | 0 | `image+0x7a` |
| function | image-address | 37 | `image+0xf2f6` |
| function | zeroed-block | 37 | `image+0xf2f6` |
| function | zero | 0 | `image+0x7a` |

Three findings, and two of them are negative:

1. **The argument register is load-bearing and binary.** Zero faults instantly; any
   readable pointer gets the full thirty-seven imports. This is the D152 finding
   reproduced deliberately rather than by accident, which is what makes it evidence.
2. **Which pointer does not matter.** The real process image and a zeroed block are
   indistinguishable. Whatever the entry point does with it, it is not reading a field
   this build gets right - or not reading one at all before it dies.
3. **The convention does not matter.** Jump and call are identical, and they differ in
   where `rsp` points and whether the count survives. So **this entry point does not read
   the stack image at all** at the point it currently reaches.

**So the image is not the wall, and `image+0xf2f6` still is.** That is worth having: it
was the leading candidate, and now it is eliminated rather than suspected. The default
stays `Process` + `ImageAddress` because it is the documented convention and the only
argument *derived* from something rather than invented, not because it measured better -
it did not.

The image itself stays regardless. It is correct, tested, and costs nothing, and the
moment a guest gets far enough for its runtime to initialise properly it will read it.
The `zero` row is what makes it more than speculation: the mechanism is provably
load-bearing, just not yet in the way it was built for.

**On the config file.** A malformed file now **fails the run** rather than falling back to
defaults. That is the important half: a typo'd setting that silently reverts leaves the
run behaving exactly as before, and the conclusion drawn is "that setting has no effect" -
a wrong answer, recorded as a measurement. Given that this whole entry is a table of
settings-versus-outcomes, that failure mode would have poisoned it.

