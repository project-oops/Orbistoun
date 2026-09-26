# Glossary

The words Orbistoun uses for its own machinery. For the vocabulary the whole collection shares -
standard ELF, and the words that mean different things in different repositories - see
[the collection's glossary](https://github.com/project-oops/OOPS/blob/main/docs/GLOSSARY.md).
For the file formats, see [SELFish's](https://github.com/project-oops/SELFish/blob/main/docs/GLOSSARY.md).

**guest**, **host**, **loader**, **target** and **implementation** are defined once for the
whole collection in
[CONVENTIONS.md section 2](https://github.com/project-oops/OOPS/blob/main/docs/CONVENTIONS.md#the-words-for-our-own-layers).
Orbistoun's own ELF-reading component is also called a loader, so where both senses apply, the
component is **the ELF loader**.

## What kind of emulator this is

**HLE (high-level emulation)** - reimplementing what a title asks for rather than simulating the
hardware and running the vendor's system software on it. A call into a platform library is
answered by Orbistoun's own code, so the surface is a list of functions, not a chip, and a wrong
answer is a bug in a function rather than a timing artefact.

**Native execution** - the guest is x86-64 and so is the host, so guest instructions execute
directly. What is intercepted is the boundary: every call out of guest code into a platform
library.

## The boundary

**Thunk** - the small piece of machine code a guest lands on when it calls an imported function,
and the dispatch behind it. One per import. It turns a guest call to
`sceKernelAllocateDirectMemory` into a call to the Rust function that implements it.

**`guest_module!`** - the macro a subsystem declares itself with: its library and its functions,
in one block. The registry resolves a NID to a declaration. Adding a system library is one such
block and one line in `modules()`.

**Stub** - a declared function with no implementation behind it. Stubs are loud by default: a
stub reports itself rather than returning a plausible zero, because a quiet wrong answer costs
far more to find than a noisy missing one. This is
[CONVENTIONS.md section 3](https://github.com/project-oops/OOPS/blob/main/docs/CONVENTIONS.md)
applied to the largest surface in the collection.

**Link plan** - everything the loader decides about a title before its first instruction runs:
where each segment sits, what each import resolves to, the thunk table, thread-local storage,
instruction rewrites and raw `syscall` sites. Linking at load applies it in the worker;
linking ahead of time stores it in the title library and reuses it; a native executable is
the same plan written as a host image (D724).

**Instruction rewrite** - an instruction the guest CPU has and the host CPU lacks, replaced at
link with an equivalent sequence (D725). It is the only change the link plan makes to guest
code.

**Worker** - the isolated process guest code executes in. A fault happens in the child; the
parent survives to write out what was learned.

## Words that mean something else next door

**Shape** - here, an instruction shape: the operand layout of an opcode in the guest GPU
instruction set. An opcode with no shape row is an error, not a skip (D123). In obSCEne, "shape"
means one of its artifact forms.

**Corpus** - here, a body of material to test against: the manifest-driven corpus of test guests
under `corpus/`, and the content-addressed shader corpus. In obSCEne, the corpus is a mined list
of name-to-NID pairs.

**Probe** - in obSCEne, that project itself. Here it usually means one of Orbistoun's own
diagnostics.

## The rest of the machinery

**Dependency spine** - the crate order, each crate depending only on those before it: core, elf,
nid, mem, hle, loader, then the execution, guest-OS, graphics and shell crates above.
[CRATES.md](CRATES.md) is the map.

**Knowledge files** - the TOML under `crates/orbistoun-hle/data/knowledge/`: stub behaviour keyed
by symbol name, read at run time, so changing what a function returns costs a relaunch rather
than a rebuild.

**Overrides** - per-title adjustments, kept as data rather than as branches in code.

**Compat** - the per-title records under `compat/`, from which every coverage figure is computed.

## Where the rest is

- [the collection's glossary](https://github.com/project-oops/OOPS/blob/main/docs/GLOSSARY.md) - standard ELF, `DT_`/`PT_`, and the cross-repository word collisions
- [SELFish](https://github.com/project-oops/SELFish/blob/main/docs/GLOSSARY.md) - NID, fSELF, PFS, packages, the generation split
- [obSCEne](https://github.com/project-oops/obSCEne/blob/main/docs/GLOSSARY.md) - checks, the census, `ps4_mode` against native
- [Prosperous](https://github.com/project-oops/Prosperous/blob/main/docs/GLOSSARY.md) - targets, chains, scan roots
