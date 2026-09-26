# Acknowledgements

Sources consulted in building Orbistoun. Recording what was consulted keeps every provenance
question answerable.

## The rule

**Reference only, never copied.** Nothing in this repository is copied from any source below.
Where prior work made something understandable it is credited here, and the implementation is
written independently (D014).

Reading another project's prose, design notes or public documentation to understand a format is
ordinary engineering. Reading its source and reproducing the structure is not, and neither is
reading vendor binaries. Anything consulted is added here in the same commit.

## Standards and open-source references

| Reference | What it informs |
|-----------|-----------------|
| FreeBSD | The guest kernel surface. Much of it is POSIX with vendor naming, so documented BSD semantics are the specification wherever an analogue exists, cited per function. |
| FreeBSD headers | ABI constants - error numbers, signal numbers, address families, socket options, open flags, kernel MIB identifiers - harvested from public headers into `abi-constants.toml`. A `#define` is an interface fact. Recorded as FreeBSD-published and target-assumed, because the target is FreeBSD-derived (D374). |
| The ELF specification | Container parsing in `orbistoun-elf`. |
| The GNU hash ELF extension | `symbol_count_from_gnu_hash` in `orbistoun-elf` implements its bucket-and-chain walk, the only way to recover a symbol count from a table that states none. |
| GNU binutils (`readelf`) | An independent oracle: import counts derived here are cross-checked against what `readelf` reports for the same file. |
| Vulkan and SPIR-V (Khronos) | The GPU translation target. |
| `OpenOrbis/OpenOrbis-PS4-Toolchain` | An open-source LLVM/Clang cross-compiler built without vendor tools. Used to build test material, and read as a source of interface facts - symbol names and argument counts - never of implementations. |
| `zerocopy`, `ash`, `clap`, `serde`, `tracing` and the rest of the dependency tree | See `Cargo.toml`; `deny.toml` holds the licence allow list. |

## GPU hardware documentation

The GPU chip vendor (AMD) publishes the instruction set architecture reference guide for each
architecture generation. Those guides are the source for the encoding table in
`crates/orbistoun-shader/data/encodings.toml`.

The command-processor packet format is likewise publicly documented, and is parsed by the Linux
kernel driver and by Mesa, both open-source software for publicly documented hardware.

This is hardware documentation from the chip vendor, describing a silicon interface it ships in
retail parts and documents for driver writers - not firmware from the platform vendor. The parts
are customised, so the published tables do not cover everything in a real shader; anything they
do not describe is counted and reported as unknown.

## Sibling projects

- **obSCEne** - the collection's conformance probe for the same platform. Its `docs/` describe
  the vendor dynamic tags, and a freestanding obSCEne probe is the control that shows a
  data-import problem is a C++ runtime problem: it imports no data symbols, where commercial
  titles do.
- **Prosperous** - the collection's remote hardware management tool and the library under it.
  `pros check` names the five services the payload work targets.
- **A private project by the same author**, outside this collection. It reaches an installed
  coding assistant by running it as a subprocess, which needs no API key, and
  `crates/orbistoun-llm/src/cli.rs` does the same. Its binary-discovery order on Windows is
  carried across with its reason: the launcher under `LOCALAPPDATA` hands off to a running
  desktop application and the caller never sees the reply, so the versioned command under
  `APPDATA` is preferred and the launcher is the last resort. The prompt goes on standard
  input rather than in the argument list, and a signed-out command is reported rather than
  signed in on the caller's behalf.

## Open-source homebrew read as guest material

**`ps5-payload-dev` (GPL-3.0).** `elfldr`, `ftpsrv`, `klogsrv`, `shsrv`, `pldmgr` and the rest
of that family are guests here, like any title: Orbistoun loads them and reports what they ask
for. What is read is what a loader reads - ELF headers, program headers, the dynamic table, the
import list and the symbol table - plus the bytes at a trap site, to tell a deliberate `ud2`
from a guest executing data.

- Nothing is copied. They are GPL-3.0 and this project is MIT/Apache-2.0. Where their published
  documentation describes an interface, it is read as prose and written out independently,
  recorded `published`, and promoted to `measured` only when a run agrees.
- No payload binary is tracked here. They live outside the repository like every other guest,
  and the provenance job fails the build on any of it.

**`PS5Dev/PS5SDK` - seen, not used.** A search for the payload calling convention returned a
summary of its argument structure. It is a different SDK from the one the payloads here are
built with, and nothing in this repository is derived from it. Its agreement with the measured
entry-point behaviour is not evidence about the SDK in use.

## Prior art in this space

Other projects in this space showed that high-level emulation of this target is tractable. Their
public writing - design notes, issue discussions, architecture descriptions - informed the shape
of this one; their source is not the basis of any code here. Three points taken from that
writing:

- High-level emulation makes incremental progress where hypervisor approaches to the same
  target do not; the reasoning is in [docs/SCOPE.md](docs/SCOPE.md).
- Hardware-probe-driven development - write a test program, run it on the hardware, encode the
  observed behaviour as a test - is the strongest per-function oracle available.
- Progress is measured by unresolved imports, not by screenshots.

Named entries:

- **`prosper`** - an independent high-level emulator for the same platform. Its public
  engineering documentation was read, specifically its packet-size audit of the vendor
  command-stream builders (`sceAgc*`): how many dwords each builder emits, how each figure was
  arrived at, and a per-row confidence. Two questions were taken from it: which builders to size
  first, and the observation that a builder emitting more dwords than the real library overruns
  the reservation of any guest that inlined the size at compile time. No figure from it is
  adopted. Every size in `crates/orbistoun-hle/data/knowledge/libSceAgc.toml` is measured by
  obSCEne against the real library and carries `known_by = "measured"`; the audit serves as a
  second opinion, and where it disagrees the divergence is recorded, not reconciled.

## Symbol names

Symbol and library names are interface identifiers - facts about an ABI. Orbistoun resolves them
by hashing candidate names generated here and matching the result against hashes in a module's
own import table, which requires reading no vendor binary (D242).

No symbol database is distributed with this repository.
