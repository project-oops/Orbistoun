# External references

Every document this project relies on that it did not produce, what was taken from each,
and how that was checked.

The point is reproducibility: anyone should be able to fetch the same material and follow
the same reasoning to the same table. A claim in this repository that cannot be traced to
either a document listed here or an experiment described here is a claim nobody can check.

## Why nothing is vendored

These documents are **linked, not copied into the repository.**

- They are published by their vendors for free download, and redistributing them is a
  copyright question this project has no need to take on.
- A checked-in PDF goes stale silently. A citation that names the document and its
  revision does not.
- Build reproducibility does not need them. Nothing here parses a document; the tables
  are derived by experiment (see below) and the documents are read by a person.

There is deliberately **no automated download**. It would add a network dependency to a
build that currently has none, and every such URL rots eventually - the failure mode is a
build that stops working for a reason unrelated to the code.

## The boundary this file exists to keep visible

Three categories, and only one of them is off-limits for licensing reasons. Conflating
them leads to refusing material that is published precisely so that developers will use
it, which is a cost with no benefit.

**Public vendor documentation and vendor-contributed open source: consumed freely.** AMD
publishes its instruction set architecture guides openly, and contributes the AMDGPU
backend to LLVM under a permissive licence. This is a silicon interface that AMD ships in
retail parts and documents for anyone to program against - it is *published for this*.
Reading it and writing code from it is ordinary engineering, it is credited, and there is
no argument for abstinence.

**Another implementation's source: not read.** Not because of licensing - much of it is
open source too - but because reimplementation-from-source converges on the original, same
constants, same odd control flow, and that convergence is evidence. It is also
*derivative*: someone else's reading of the hardware, inheriting their mistakes with none
of their working. This project derives its own and keeps the proof. See
[PROVENANCE.md](PROVENANCE.md) and principle 1 in [CLAUDE.md](../CLAUDE.md).

**The platform vendor's own binaries and firmware: never.** Not public, not licensed, not
ambiguous.

The distinction is not academic: it decides whether this work can ever be shared. Getting
it wrong in the permissive direction ends the project; getting it wrong in the restrictive
direction just makes it slower for no reason, and that has happened - see the note on
LLVM's tables below, which is an engineering argument that has been mistaken for a
licensing one.

## Instruction set

**AMD publishes an ISA reference guide per GPU generation**, covering instruction
encodings, opcode numbering, and per-instruction behaviour. They are distributed through
AMD's developer documentation and GPUOpen.

The generation this project targets is **RDNA2** - the hardware's GPU is an RDNA2
derivative - and the reference toolchain names it `gfx1030`. The previous-generation
hardware is GCN, `gfx900`, and is a *nice to have* rather than a requirement.

The target lives in one place, [`orbistoun-gen`'s `target` module](../crates/orbistoun-gen/src/target.rs), and every
generator and probe script reads it from there. It was four separate constants until
D139, which is part of why it went months pointing at the wrong generation with nothing
saying so.

**The document actually used:** *"RDNA 2" Instruction Set Architecture: Reference Guide*,
AMD document **70648**, 283 pages, retrieved 2026-08-20 from AMD's documentation portal.
The encoding family rows in `crates/orbistoun-shader/data/encodings.toml` come from its
chapter 13, *Microcode Formats*.

> **Fetching it:** the portal is a JavaScript application, so the readable page
> (`docs.amd.com/v/u/en-US/rdna2-shader-instruction-set-architecture`) does not serve the
> PDF directly; the download link on it points at
> `docs.amd.com/api/khub/documents/Et~wpu9g~Ffl7d9q0QZ~Og/content`. The older
> `amd.com/system/files/TechDocs/...` URL that GPUOpen still advertises now redirects into
> that portal and returns HTML. **Neither URL is load-bearing** - AMD reorganises this
> site, and the document number and title above are what to search for when they rot.
>
> It is **not vendored**, for the reasons at the top of this file. It is read by a person
> and cited here.

### The document is a strong reference, not an infallible one

Its field table for the LDS format gives that format's opcode as bits `[24:17]`. **It is
`[25:18]`.** Under the document's own field positions `ds_read_b32` decodes as opcode 108
and `ds_write_b32` as 26; the document's *own opcode table*, forty pages earlier, says
they are 54 and 13. Bits `[25:18]` produce exactly those.

This was not caught by reading carefully. The encoding solver solved the field from
assembled bytes and disagreed with the document, and the disagreement is what prompted
checking the document against itself. That is the whole reason a second, independent
derivation is worth having even when an authoritative specification exists - and it is
the pairing this file describes, running in the direction nobody plans for: the
experiment corrected the document.

### Reproducing the tables

The tables below are derived by running a reference assembler; that assembler is not on
every host this project is developed on, and for a long time the machine that had it was
undocumented - so the tables could be read by anyone and re-derived by nobody.

```bash
sh tools/toolchain/setup.sh                                   # a VM with the toolchain
sh tools/toolchain/run.sh cargo run --release -p orbistoun-gen -- operands
sh tools/toolchain/run.sh cargo run --release -p orbistoun-gen -- fixtures
```

The generators refuse to fall back to whatever assembler happens to be on the host's
`PATH`. A table whose provenance is "some assembler, somewhere" is the thing this file
exists to prevent.

Used for, and how each is verified:

| What | Where it lives | How it is checked |
|---|---|---|
| Encoding families - mask, value, opcode position, instruction width | `crates/orbistoun-shader/data/encodings.toml` | Differentially, against a reference disassembler over 126 instructions. This found one row that was **wrong**, not merely unverified. |
| The unified source-operand numbering | `crates/orbistoun-shader/data/operands.toml` | Same differential test, operand by operand. |
| Per-opcode operand layouts - which field each operand sits in | `crates/orbistoun-shader/data/opcode-operands.toml` | Solved from assembled probes rather than transcribed, and refused rather than guessed when two readings both fit. The same differential test then checks every operand it produces against the reference. |
| Instruction names | `crates/orbistoun-shader/data/mnemonics.toml` | Observed only: a compiler emitted the instruction and a disassembler named it. It covers what the fixtures exercise and nothing else, so an unobserved name is absent rather than guessed. |
| Typed-buffer formats - component count, width and type per code | `crates/orbistoun-shader/data/buffer-formats.toml` | Measured in both directions. Each code is assembled and the field read back to confirm it; then each candidate *name* is assembled to recover the default, which the disassembler never prints. Codes with no meaning are absent rather than approximated. |
| Per-instruction behaviour, including side effects on hidden state | `crates/orbistoun-translate/src/model.rs` | Executed on a real GPU against expected values. **Side effects are the exception** - see below. |

### What the documents are needed for that experiment cannot supply

Three things, and they are the reason this file matters rather than being bookkeeping.

**Hidden side effects.** Most scalar instructions write the condition code as well as
their destination, and what it means differs by family - non-zero result for the logical
operations, signed overflow for the arithmetic ones, and one instruction that does not
write it at all. None of this is visible in the encoding, in the operand layout, or in
any test that checks destinations. It was missed once (D129) and had to be looked up.

**The division sequence.** `v_div_scale_f32`, `v_div_fmas_f32` and `v_div_fixup_f32`
implement float division together, and need exponent thresholds and a table of
special-case substitutions. They were refused rather than guessed (D124) until the
published reference for this generation supplied all three - which is this section's
point made twice over: the document was needed, and once fetched it settled the question
immediately. **All three are translated now**, and this paragraph is kept as the example
rather than as an open item.

**Correcting a row the differential test rejects.** The reference detects an error; the
document supplies the fix. Reversing that - reading the reference's own tables for the
right value - is the derivation this project refuses (D085).

## Reference implementations, used only as oracles

**LLVM's AMDGPU backend**, through the `llvm-mc` assembler and `llvm-objdump`
disassembler.

Used **as programs, never as source.** Two ways:

- `orbistoun-gen operands` assembles instructions with varied operands and solves
  each opcode's operand fields from the bytes that come out. Sixty-two opcodes' layouts
  are derived this way rather than transcribed. This is observation of behaviour.
- `orbistoun-gen fixtures` compiles shaders and records where the disassembler says
  each instruction begins. `crates/orbistoun-shader/tests/differential.rs` asserts our
  boundaries match.

LLVM's instruction definitions are machine-readable and it would be straightforward to
generate tables from them. **That is exactly what is not done**, and the reason is in the
boundary above.

**Khronos SPIR-V specification** - the output format. Opcode numbers and structural rules
in `crates/orbistoun-spirv`. Verified by `spirv-val`, which is the specification's own
validator rather than this project's opinion of itself.

**Vulkan specification** - `crates/orbistoun-gpu-vulkan`, through the `ash` bindings.

### Why the tables are not generated from LLVM's TableGen files

This comes up, and it has been raised as a *provenance* objection at least once. It is not
one, and answering it as though it were leads to the wrong conclusion in both directions.

`llvm/lib/Target/AMDGPU/*.td` is machine-readable, complete, covers this target, and is
permissively licensed vendor-contributed open source. Nothing in the boundary above
forbids reading it. Anyone who says otherwise is applying the emulator rule to something
that is not an emulator.

The reason is **oracle independence**, and it is an engineering constraint rather than a
legal one.

The two sources here have deliberately different jobs. The AMD document *supplies values*.
LLVM *detects errors*, through its behaviour as a black box - assemble something, look at
the bytes. Generating our table from LLVM's tables collapses those into one source, and
the differential test stops being a test: it can only ever confirm that LLVM agrees with
itself.

That is not hypothetical. The LDS opcode field is at bits `[25:18]` and the document's
field table says `[24:17]`. The generator solved it from assembled bytes, disagreed with
the document, and the document's own opcode table forty pages earlier settled it. Two
independent sources, one wrong, caught. Had the table been generated from `.td`, both
sides of that comparison would have been LLVM.

**Where it would earn its place is as a third source.** Cross-checking rows the document
and the assembler both leave ambiguous, or supplying what neither gives cheaply - the
hidden condition-code side effects and the division thresholds above are both `BLOCKED`
in `model.rs` for exactly that reason, and a machine-readable table may well settle them.
That is additive rather than circular, and it would want an
[ACKNOWLEDGEMENTS.md](../ACKNOWLEDGEMENTS.md) entry and a `published` attribution like
anything else.

**It has no consumer today, and that is worth stating so nobody goes looking for one.**
The obvious candidates were the hidden condition-code side effects and the division
thresholds - both were named here as open when this was written, and both are in fact
implemented. The one remaining `BLOCKED` entry is `exp`, and no table can supply what it
is missing: it needs a render target to export to, which is a design decision about this
project, not a fact about the hardware.

The rule, stated once: **LLVM may check a table and may cross-check a fact; it may not be
the thing the table is generated from.**

## Operating system

**FreeBSD source**, for the target's C library. Lawful, citable, and the strongest
reference available for that layer - much of it is POSIX with vendor naming. Cited at the
point of use rather than listed here, because the relevant file differs per function.

## Open homebrew, read as guests rather than as sources

**[ps5-payload-dev](https://github.com/ps5-payload-dev), GPL-3.0.** `elfldr`, `ftpsrv`,
`klogsrv`, `shsrv`, `pldmgr`.

**What was taken:** nothing but what a loader takes. ELF and program headers, the dynamic
table, `DT_NEEDED`, the import list, the symbol table, and eight bytes at one address to
distinguish a compiler-emitted `ud2` from a guest running in non-code.

**How it was checked:** every import count was re-derived independently with `readelf` and
agreed exactly on all five (D305). The entry-contract findings are runs of the binaries
under two diagnostics, reproducible by anyone with the same files and
`[entry] argument = "sentinels"` or `"answering"` (D308).

**What was deliberately not taken:** their source. They are GPL-3.0 and this project is
MIT/Apache-2.0, so copying even an interface declaration is a licensing problem separate
from the provenance one. Their *published documentation* of the payload handoff ABI is
readable as prose under principle 1 - and anything taken that way is recorded `published`,
written out independently, and promoted to `measured` only once a run agrees.

**Where the binaries live:** outside this repository, like every other guest. The
`provenance` job fails the build on any of them being tracked.

## The GNU hash ELF extension

**What was taken:** the layout - bucket count, symbol bias, bloom word count and shift,
then the bloom words, buckets and chain - and the fact that the chain's low bit terminates
a run. Enough to recover a symbol count from a table that, unlike `DT_HASH`, states none.

**How it was checked:** `symbol_count_from_gnu_hash` has unit tests built from
hand-assembled tables where the answer is known by construction, including the
every-bucket-zero case and two malformed ones. Then against real files: twenty-one of
twenty-three payloads carry only this table, and the counts it produces match `readelf`
exactly on all of them (D305).

## What is not a reference

**Other projects in this space.** Reading their prose, design notes or public
documentation to understand a format is ordinary engineering and anything consulted is
credited in [ACKNOWLEDGEMENTS.md](../ACKNOWLEDGEMENTS.md). Reading their source is the
convergence problem arriving by another route, and is not done.

> This is about *derivative* work, not about open source. Another emulator's decode table
> is one team's reading of the hardware, and adopting it inherits their mistakes with none
> of their working - which is the opposite of what this project is for. The **vendor's**
> own published documentation and open-source contributions are a different thing
> entirely and are used freely; see the boundary section at the top. An objection to
> reading AMD's material, or LLVM's AMDGPU backend, on the grounds that emulator source
> is off-limits is a category error.

**Anything observed from the hardware or a title.** Behaviour observed at runtime is
evidence and may be recorded; bytes read out of a vendor binary are not, and the
provenance guard in `bin/orbistoun` fails the build on the file types they arrive in.

## FreeBSD headers - the ABI constants

**What it is.** The same checkout as below, widened. It was cloned shallow and
`blob:none`-filtered with a sparse pattern covering only `lib/` - which was right while the
only thing being taken was names, and cost 22 MB. Adding `sys/sys`, `sys/netinet` and
`include` took it to 25 MB.

**What was taken.** `#define NAME <number>` from six headers - `errno.h`, `signal.h`,
`socket.h`, `netinet/in.h`, `fcntl.h`, `sysctl.h` - **911 constants**, generated by `orbistoun-gen constants <checkout> --revision <commit>`
into `crates/orbistoun-hle/data/abi-constants.toml` with the commit they came from
(`ee81cd1d8f5596a6ab4c8eb29009405572cc162b`). No function bodies, no structure layouts.

**Why this became necessary.** The names harvest was scoped for the *naming* loop, and the
work has moved to *implementing*. Eight functions were written before this and the gap cost
three answers: `SIGPIPE` was recovered from the guest's own call argument, a `sysctl` MIB
was recorded as a number with its meaning left open, and `errno` was left unset because
`ENOENT`'s value was not derivable from anything lawful here (D350).

Sockets would have been far worse. `socket(AF_INET, SOCK_STREAM, 0)` cannot be mapped onto a
host socket without knowing what those are, and a wrong value there creates the wrong kind
of socket - a silent, late failure of exactly the shape principle 3 exists to prevent.

**How it was checked.** Four constants were confirmed against measurements taken **before**
the harvest existed, and neither was derived from the other:

| constant | header says | measured independently |
|---|---|---|
| `SIGPIPE` | 13 | `klogsrv` passed `0xd` to `signal` |
| `CTL_KERN` | 1 | MIB[0] in the dumped `sysctl` call |
| `KERN_PROC` | 14 | MIB[1] |
| `KERN_PROC_PROC` | 8 | MIB[2], from a caller the symbol table names `find_pid` |

`KERN_PROC_PROC` is commented *"only return procs"* - so `[1, 14, 8, 0]` is *enumerate all
processes*, which is what a function called `find_pid` would ask for. The measurement and
the citation agree completely.

**The distinction that has to survive.** These are **FreeBSD's numbers, not the target's.**
The target is FreeBSD-*derived*, which is why they are worth having and also why they are
not facts about it. Each is `published` about FreeBSD and `assumed` about the guest, the
data file says so at the top, and a guest passing a value that disagrees is what would show
it.

**Proved on every run.** `./bin/orbistoun check` regenerates the table from the checkout and
diffs it, so a hand-edited value, a deleted constant or a header naming a revision the
checkout is not at all fail (D354). The revision is **asked of the checkout**, never read
out of the file - a revision the file states is a claim, and the first version of this gate
re-derived the file using that claim, so a header edited to name a different source matched
itself and passed. Where no checkout is present the step warns and passes, saying so.

**Not retyped.** `orbistoun_libc::abi_constant` reads the file. A value copied into Rust
would be untraceable - a reader could no longer tell a harvested constant from a remembered
one, which is the whole distinction `known_by` keeps. A test pins `SOL_SOCKET` at `0xffff`
specifically because it is `1` on several other platforms, so a table built from recall
would differ there first.

## FreeBSD source - the C library word list

**What it is.** `github.com/freebsd/freebsd-src`, BSD-2-Clause, fetched at commit
`2ff0ca5272c8c2bb038a565949d5bd5c4726c704` (`main`, 2026-08-20).

**What was taken.** Symbol *names*, and nothing else. Every `Symbol.map` under
`lib/libc`, `lib/libthr`, `lib/msun` and `lib/libutil` - 46 files - read for the symbols
each library declares it exports. 2,497 names, written to
`crates/orbistoun-names/data/standard.txt`.

**No source code was read**, and none is vendored. A `Symbol.map` is a linker version
script: a list of names and the version each appeared in. It contains no implementation,
no structure layouts, and no constants.

**Why this source.** The target's C library is FreeBSD-derived, so these are the names its
interface actually uses. A symbol map is the project's own authoritative statement of what
it exports - stronger than a person's recollection of what ISO C contains, which is what
this replaced.

**How to reproduce it.** Exactly, from nothing:

```bash
git clone --filter=blob:none --sparse --depth 1 https://github.com/freebsd/freebsd-src
cd freebsd-src && git sparse-checkout set lib/libc lib/libthr lib/msun lib/libutil
cargo run -p orbistoun-names --example harvest -- . "github.com/freebsd/freebsd-src @ $(git rev-parse HEAD)"
```

The generated file carries that revision in its own header, so a reader never has to come
here to find out where it came from.

**How it is checked.** Names are not trusted; they are *tested*. Each is hashed and matched
against the import table a real module declares, and only a collision counts. A wrong name
matches nothing, so a bad entry in this list costs a wasted hash and cannot introduce a
false result.

**What replaced.** A hand-written list of about 470 names, typed from knowledge of ISO C
and POSIX. Correct as far as it went and impossible to audit - "somebody wrote these down"
is not a citation. This is.

### A rule that cost the most important name in the corpus

The first harvest skipped every symbol beginning with an underscore, on the reasoning that
reserved names are implementation detail. That excluded **`__cxa_atexit`** - the single
most-called import across every title examined, 53.5% of all calls.

Programs import reserved names constantly, and the C++ ABI is nothing but reserved names.
The filter now keeps them, and relies on the distinction the format itself makes: FreeBSD
marks implementation detail with `FBSDprivate_*` version blocks, which are skipped. A rule
the source states beats a rule we invented (D126).


## A conformance run on a target console

**What it is.** `data/hardware/ps5-full.txt` in the sibling conformance-probe repository - the
complete suite, 521 checks in 28 sections, recorded 2026-08-30. Its header names the artefact
that produced it, the console state it ran under, and the ten checks excluded because they end
the process.

**What was taken from it.** Values, and only values: the error encoding, the direct memory size,
the counter frequency, the microsecond unit, the query structure's third field and its accepted
flags and sizes, the default mutex attribute type, and the type-dependent behaviour of
`trylock`. Each is recorded against the function it belongs to with this file cited, and D398
lists them.

**How it is checked.** Two ways, both cheap. The encoding is pinned as a test that names which
observation it contradicts if changed, so it cannot drift silently. And the frequency
cross-checks itself inside the run - a sleep of known length advanced two different clocks, and
the ratio between them agrees with the frequency the console reported to four significant
figures. A single reported number would have been one observation; that makes it two.

**What was deliberately not taken.** Nothing was copied - no structure declaration, no header,
no code. The file records what a machine answered, and what is written down here is what it
answered, in this project's own words and shapes.


## The process-parameter block layout

**What it is.** The `PT_SCE_PROCPARAM` segment's structure - a size, a `"ORBI"` magic, an entry
count, two SDK-version words, then pointers to the libc, kernel-memory, and one further parameter
block. Read by `crates/orbistoun-elf/src/procparam.rs`.

**Where it came from.** Two lawful sources, agreeing. The **OpenOrbis PS4 ELF specification** (the
open-source toolchain already credited in `ACKNOWLEDGEMENTS.md`) documents the header - the magic,
the entry count, the fixed size. The sibling conformance probe's `crt.c` builds exactly this
structure to launch on real hardware and cites that specification; two of its offsets are
**hardware-confirmed** rather than only documented - obSCEne's D219 records a console faulting on a
write through a null pointer left at the block's `+0x40` slot, which pins the kernel-memory pointer
to that offset. No vendor header or SDK was read.

**What was taken from it.** The field offsets, and only those: `size` at `+0x00`, magic at `+0x08`,
entry count at `+0x0c`, SDK versions at `+0x10`/`+0x14`, and the three pointers at `+0x38`/`+0x40`/
`+0x48`. The offsets are declared once in `procparam.rs` with this provenance in the module note.

**How it is checked.** Against real material as an oracle (SELFish principle 2, shared): the reader
was run over every resident title through `orbistoun-cli inspect`. Each returns a coherent header -
magic `"ORBI"`, entry count 5, a distinct plausible SDK version per title - and obSCEne's own eboot,
whose `crt.c` demonstrably fills the three pointers, resolves them to real addresses, which is what
confirms the reader locates and relocates the block correctly rather than reading coincidental bytes
(D442 update).

**What was deliberately not taken.** The *contents* of the blocks the pointers lead to. obSCEne
supplies its kernel-memory block sized but empty, so its build establishes that the block exists and
how large it is, but not where any field sits inside it. Reading a field out of a real title's block
would be deriving a layout from material, which the shared provenance rule forbids - so the reader
reports where the pointer leads and no further.
