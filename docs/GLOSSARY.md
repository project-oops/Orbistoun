# Glossary

The words orbistoun uses for its own machinery. For the vocabulary the whole collection shares
- standard ELF, and the words that mean different things in different repositories - see
[the collection's glossary](https://github.com/project-oops/OOPS/blob/main/docs/GLOSSARY.md).
For the file formats, see [SELFish's](https://github.com/project-oops/SELFish/blob/main/docs/GLOSSARY.md).

**guest**, **host**, **loader**, **target** and **implementation** are defined once for all
five repositories in
[CONVENTIONS.md §2](https://github.com/project-oops/OOPS/blob/main/docs/CONVENTIONS.md#the-words-for-our-own-layers)
and are not repeated here. The one to watch: orbistoun calls its own ELF-reading component
"the loader" as well, so where both senses are live, write **the ELF loader** for the
component.

## What kind of emulator this is

**HLE - high-level emulation.** Rather than simulating the hardware and running the vendor's
own system software on it, orbistoun reimplements what a title *asks for*. A call into a
platform library is answered by our own code. The consequence is that the surface is a list of
functions, not a chip - and that a wrong answer is a bug in a function rather than a timing
artefact.

**Guest code runs natively.** The guest is x86-64 and so is the host, so instructions are not
translated - they are executed. What has to be intercepted is the boundary: every call *out* of
guest code into a platform library.

## The boundary

**Thunk** - the small piece of machine code a guest lands on when it calls an imported
function, and the dispatch behind it. One per import. This is the mechanism that turns "the
guest called `sceKernelAllocateDirectMemory`" into "our Rust function ran".

**`guest_module!`** - the macro a subsystem declares itself with: its library and its
functions, in one block. The registry then resolves a NID to a declaration. Adding a system
library is one such block and one line elsewhere.

**Stub** - a declared function with no real implementation behind it yet. Stubs here are
**loud by default**: a stub says so rather than returning a plausible zero, because a quiet
wrong answer costs far more to find than a noisy missing one. This is
[CONVENTIONS.md §3](https://github.com/project-oops/OOPS/blob/main/docs/CONVENTIONS.md)
applied to the largest surface in the collection.

**Worker** - the isolated process guest code executes in. A fault happens in the child; the
parent survives to write out what was learned.

## Words that mean something else next door

**Shape** - here, an **instruction** shape: the operand layout of an opcode in the graphics
instruction set. An opcode with no shape row is an error, not a skip (D123), and a shape has
two costs that rank differently (D264). In obSCEne, "shape" means one of its artifact forms -
an unrelated sense.

**Corpus** - here, a body of material to test against: the manifest-driven test corpus of
titles (D042), and the content-addressed shader corpus (D088). "The corpus is the oracle"
(D303). In obSCEne, the corpus is a mined list of name-to-NID pairs.

**Probe** - in obSCEne, that project itself. Here it usually means one of its own diagnostics.

## The rest of the machinery

**Dependency spine** - the crate order, each depending only on those before it: core, elf, nid,
mem, hle, loader, then execution, guest-OS, graphics and shell crates above. The split is not
organisational; it is what keeps concerns from leaking. [CRATES.md](CRATES.md) is the map.

**Knowledge files** - the TOML under `crates/orbistoun-hle/data/knowledge/`. Stub behaviour
keyed by symbol name, read at runtime, so bisecting what a function should return costs a
relaunch rather than a rebuild.

**Overrides** - per-title adjustments, kept as data rather than as branches in code.

**Compat** - the per-title records under `compat/`, which is what a coverage claim is computed
from.

## Where the rest is

- [the collection's glossary](https://github.com/project-oops/OOPS/blob/main/docs/GLOSSARY.md) - standard ELF, `DT_`/`PT_`, and the cross-repository word collisions
- [SELFish](https://github.com/project-oops/SELFish/blob/main/docs/GLOSSARY.md) - NID, fSELF, PFS, packages, the generation split
- [obSCEne](https://github.com/project-oops/obSCEne/blob/main/docs/GLOSSARY.md) - checks, the census, `ps4_mode` against native
- [Prosperous](https://github.com/project-oops/Prosperous/blob/main/docs/GLOSSARY.md) - targets, chains, scan roots
