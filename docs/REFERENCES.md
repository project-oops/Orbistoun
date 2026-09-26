# External references

Every document this project relies on that it did not produce, what was taken from each,
and how it was checked. Anyone can fetch the same material and follow the same reasoning to
the same table. A claim that traces to neither a document nor an experiment listed here is a
claim nobody can check.

## Documents are linked, not vendored

- They are published by their authors for free download; redistributing them is a copyright
  question this project does not need to take on.
- A citation naming the document and its revision does not go stale silently; a checked-in
  copy does.
- The build does not need them. Nothing parses a document: tables are derived by experiment
  and the documents are read by a person.

There is no automated download, so the build has no network dependency on a URL that can
move.

## The boundary

| Material | Rule |
|---|---|
| Public GPU documentation and vendor-contributed open source (the AMD ISA guides, LLVM's AMDGPU backend) | used freely and credited: it documents a silicon interface for anyone to program against |
| Another implementation's source | not read: reimplementation from source converges on the original, and the result inherits someone else's reading of the hardware |
| The platform vendor's binaries and firmware | never |

Other projects' prose, design notes and public documentation may be read to understand a
format, and are credited in [ACKNOWLEDGEMENTS.md](../ACKNOWLEDGEMENTS.md). Behaviour
observed from the hardware or a title is evidence and may be recorded; bytes read out of a
vendor binary are not, and the provenance guard in `bin/orbistoun` fails the build on the
file types they arrive in. Name provenance is in [PROVENANCE.md](PROVENANCE.md).

## GPU instruction set reference

**Source.** *"RDNA 2" Instruction Set Architecture: Reference Guide*, AMD document 70648,
from AMD's documentation portal. The hardware's GPU is an RDNA2 derivative, `gfx1030` in
LLVM's naming; the previous generation is GCN, `gfx900`. The target is defined once, in
[`orbistoun-gen`'s `target` module](../crates/orbistoun-gen/src/target.rs), and every
generator and probe script reads it from there (D139).

The portal is a JavaScript application: the readable page
`docs.amd.com/v/u/en-US/rdna2-shader-instruction-set-architecture` links the PDF at
`docs.amd.com/api/khub/documents/Et~wpu9g~Ffl7d9q0QZ~Og/content`. Neither URL is
load-bearing; the document number and title are what to search for.

**Taken.** The encoding family rows in `crates/orbistoun-shader/data/encodings.toml`, from
chapter 13, *Microcode Formats*. Condition-code side effects of scalar instructions, which
differ by family and are invisible in the encoding, the operand layout and any test of
destinations (D129). The exponent thresholds and special-case substitutions of the division
sequence `v_div_scale_f32`, `v_div_fmas_f32`, `v_div_fixup_f32`.

**Erratum.** The field table for the LDS format gives the opcode at bits `[24:17]`; it is at
`[25:18]`. Only `[25:18]` yields the document's own opcode table values for `ds_read_b32`
(54) and `ds_write_b32` (13). The encoding solver found it by solving the field from
assembled bytes.

**Checked.** Differentially, against LLVM used as an oracle (below). When the differential
test rejects a row, the document supplies the correction; reading LLVM's tables for the
value is the derivation this project refuses (D071).

## Tables derived by experiment

The generators run LLVM's reference assembler and disassembler for the target GPU inside the
toolchain VM, and refuse to fall back to whatever assembler is on the host `PATH`:

```bash
sh tools/toolchain/setup.sh                                   # a VM with the toolchain
sh tools/toolchain/run.sh cargo run --release -p orbistoun-gen -- operands
sh tools/toolchain/run.sh cargo run --release -p orbistoun-gen -- fixtures
```

| What | Where it lives | How it is checked |
|---|---|---|
| Encoding families: mask, value, opcode position, instruction width | `crates/orbistoun-shader/data/encodings.toml` | differentially, against the reference disassembler |
| Unified source-operand numbering | `crates/orbistoun-shader/data/operands.toml` | the same differential test, operand by operand |
| Per-opcode operand layouts | `crates/orbistoun-shader/data/opcode-operands.toml` | solved from assembled probes, refused when two readings both fit; every operand then checked by the differential test |
| Instruction names | `crates/orbistoun-shader/data/mnemonics.toml` | observed only: a compiler emitted the instruction and the disassembler named it; an unobserved name is absent |
| Typed-buffer formats: component count, width and type per code | `crates/orbistoun-shader/data/buffer-formats.toml` | each code assembled and read back, and each candidate name assembled to recover the default the disassembler never prints; codes with no meaning are absent |
| Per-instruction behaviour, including hidden-state side effects | `crates/orbistoun-translate/src/model.rs` | executed on a real GPU against expected values; side effects come from the ISA guide |

## LLVM's AMDGPU backend, as an oracle

**Source.** `llvm-mc` and `llvm-objdump` for the AMDGPU target.

**Taken.** Behaviour only, observed as programs:

- `orbistoun-gen operands` assembles instructions with varied operands and solves each
  opcode's operand fields from the output bytes.
- `orbistoun-gen fixtures` compiles shaders and records where the disassembler says each
  instruction begins; `crates/orbistoun-shader/tests/differential.rs` asserts our
  boundaries match.

**Not taken.** The TableGen definitions in `llvm/lib/Target/AMDGPU/*.td`. They are
permissively licensed and nothing in the boundary forbids reading them; the constraint is
oracle independence. The ISA guide supplies values and LLVM detects errors as a black box.
Generating a table from LLVM's tables makes the differential test compare LLVM with itself,
and the LDS erratum above is the kind of disagreement it would stop catching.

**Rule.** LLVM may check a table and cross-check a fact; it is never the thing a table is
generated from. Used as a third source for a fact the guide and the assembler both leave
open, it needs an [ACKNOWLEDGEMENTS.md](../ACKNOWLEDGEMENTS.md) entry and a `published`
attribution.

## SPIR-V and Vulkan specifications

**SPIR-V specification (Khronos).** Opcode numbers and structural rules in
`crates/orbistoun-spirv`. Checked by `spirv-val`, the specification's own validator.

**Vulkan specification (Khronos).** `crates/orbistoun-gpu-vulkan`, through the `ash`
bindings.

## FreeBSD source: the C library word list

**Source.** `github.com/freebsd/freebsd-src`, BSD-2-Clause. The revision is in the header of
`crates/orbistoun-names/data/standard.txt`.

**Taken.** Symbol names only: every `Symbol.map` under `lib/libc`, `lib/libthr`, `lib/msun`
and `lib/libutil`, written to `crates/orbistoun-names/data/standard.txt`. A `Symbol.map` is a
linker version script, a list of names and versions; it holds no implementation, layouts or
constants. Reserved names (leading underscore) are kept, since programs import them and the
C++ ABI consists of them; FreeBSD's `FBSDprivate_*` version blocks mark implementation
detail and are skipped (D126).

**Reproduce.**

```bash
git clone --filter=blob:none --sparse --depth 1 https://github.com/freebsd/freebsd-src
cd freebsd-src && git sparse-checkout set lib/libc lib/libthr lib/msun lib/libutil
orbistoun-cli harvest . --revision "$(git rev-parse HEAD)"
```

**Checked.** Each name is hashed and matched against the imports a real module declares;
only a collision counts. A wrong name matches nothing, so a bad entry costs a wasted hash and
cannot introduce a false result.

Elsewhere, FreeBSD source is cited at the point of use, because the relevant file differs
per function.

## FreeBSD headers: the ABI constants

**Source.** The same checkout, with `sys/sys`, `sys/netinet` and `include` added to the
sparse pattern. The commit is in the header of `crates/orbistoun-hle/data/abi-constants.toml`.

**Taken.** `#define NAME <number>` and its trailing comment from `errno.h`, `signal.h`,
`socket.h`, `netinet/in.h`, `fcntl.h` and `sysctl.h`, generated by
`orbistoun-gen constants <checkout> --revision <commit>` into
`crates/orbistoun-hle/data/abi-constants.toml`. No function bodies, no structure layouts, no
expressions. These are FreeBSD's numbers, not the target's: each is `published` about
FreeBSD and `assumed` about a guest, and the file says so at the top.

**Checked.** Against independent measurements:

| Constant | Header | Measured |
|---|---|---|
| `SIGPIPE` | 13 | `klogsrv` passed `0xd` to `signal` |
| `CTL_KERN` | 1 | MIB[0] of a dumped `sysctl` call |
| `KERN_PROC` | 14 | MIB[1] |
| `KERN_PROC_PROC` | 8 | MIB[2], from a caller the symbol table names `find_pid` |

`./bin/orbistoun check` regenerates the table from the checkout and diffs it, so a
hand-edited value, a deleted constant or a header naming a revision the checkout is not at
all fail. The revision is asked of the checkout, never read from the file (D352). Without a
checkout the step warns and passes, saying so.

`orbistoun_libc::abi_constant` reads the file; no value is retyped into Rust, so a harvested
constant stays distinguishable from a remembered one. A test pins `SOL_SOCKET` at `0xffff`,
which is `1` on several other platforms.

## GNU hash ELF extension

**Taken.** The layout - bucket count, symbol bias, bloom word count and shift, then the bloom
words, buckets and chain - and the rule that the chain's low bit ends a run. Enough to
recover a symbol count from a table that, unlike `DT_HASH`, states none.

**Checked.** `symbol_count_from_gnu_hash` has unit tests on hand-assembled tables with
known answers, including the every-bucket-zero case and two malformed ones, and its counts
match `readelf` on real payloads (D305).

## Open homebrew binaries, read as guests

**Source.** [`ps5-payload-dev`](https://github.com/ps5-payload-dev), GPL-3.0: `elfldr`,
`ftpsrv`, `klogsrv`, `shsrv`, `pldmgr`. The binaries live outside the repository like every
guest; the `provenance` job fails the build if one is tracked.

**Taken.** What a loader takes: ELF and program headers, the dynamic table, `DT_NEEDED`, the
import list, the symbol table, and eight bytes at one address to distinguish a
compiler-emitted `ud2` from a guest running in non-code.

**Checked.** Import counts re-derived independently with `readelf` agree exactly (D305). The
entry-contract findings are runs of the binaries under `[entry] argument = "sentinels"` or
`"answering"`, reproducible by anyone with the same files (D365).

**Not taken.** Their source. It is GPL-3.0 and this project is MIT/Apache-2.0, so even an
interface declaration is not copied. Their published documentation of the payload handoff
ABI is readable as prose; anything taken from it is recorded `published`, written
independently, and promoted to `measured` when a run agrees.

## obSCEne conformance run

**Source.** `data/hardware/ps5-full.txt` in obSCEne: the complete conformance suite run on
the hardware. Its header names the artefact that produced it, the machine state it ran
under, and the checks excluded because they end the process.

**Taken.** Values only: the error encoding, the direct memory size, the counter frequency,
the microsecond unit, the query structure's third field with its accepted flags and sizes,
the default mutex attribute type, and the type-dependent behaviour of `trylock`. Each is
recorded against its function with this file cited (D398).

**Checked.** The error encoding is pinned by a test that names the observation it
contradicts if changed. The counter frequency cross-checks inside the run: a sleep of known
length advanced two clocks, and their ratio agrees with the reported frequency to four
significant figures.

**Not taken.** No structure declaration, header or code; the values are written in this
project's own words and shapes.

## Platform library directories

**Source.** `docs/PLATFORM_LIBRARIES.md` and `data/hardware/ps5-sprx-manifest.tsv` in
obSCEne: the platform's system modules across three directories, with each one's privilege
tier.

| Directory | Tier |
|---|---|
| `/system/common/lib/` | mapped into every game sandbox |
| `/system_ex/common_ex/lib/` | system applications |
| `/system/priv/lib/` | privileged services |

**Taken.** The three directory paths, as the table `sceKernelLoadStartModule` checks.
`/system_ex` is not under `/system`. Privilege tiers and credential values are not taken:
orbistoun does not sandbox.

**Checked.** The two `/system` directories are measured: obSCEne's `110-modules/load` asks
for both. `/system_ex` rests on the manifest alone, and is recorded that way so an inferred
path stays distinguishable from a printed one. The hardware answers `0x8002_0002` both for a
firmware module and for a path that does not exist (`060-module/load-rejects-missing`), so a
wrong directory list is not observable through that error.

## Process-parameter block layout

**Source.** The `PT_SCE_PROCPARAM` segment's structure, read by
`crates/orbistoun-elf/src/procparam.rs`. Two lawful sources agree: the OpenOrbis toolchain's
ELF specification (credited in [ACKNOWLEDGEMENTS.md](../ACKNOWLEDGEMENTS.md)) documents the
header - magic, entry count, fixed size - and obSCEne's `crt.c` builds the structure to
launch on the hardware and cites that specification. On hardware, a null pointer left at `+0x40` faults on the first write through it,
which fixes the kernel-memory pointer at that offset. No vendor header or SDK is read.

**Taken.** Field offsets only: `size` at `+0x00`, magic `"ORBI"` at `+0x08`, entry count at
`+0x0c`, SDK versions at `+0x10` and `+0x14`, and the libc, kernel-memory and further
parameter-block pointers at `+0x38`, `+0x40` and `+0x48`. They are declared once in
`procparam.rs`.

**Checked.** `orbistoun-cli inspect` over resident titles returns a coherent header for each:
magic `"ORBI"`, entry count 5, a plausible SDK version per title. obSCEne's own
eboot, whose `crt.c` fills the three pointers, resolves them to real addresses, confirming
the reader locates and relocates the block (D444).

**Not taken.** The contents of the blocks the pointers lead to. obSCEne supplies its
kernel-memory block sized but empty, so it establishes the block's existence and size but no
field positions; reading a field from a title's block would derive a layout from material.
The reader reports where each pointer leads and no further.
