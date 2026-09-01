# Acknowledgements

Work that informed this project. Listed because credit is owed, and because openly
recording what was consulted is better provenance hygiene than silence.

## The rule

**Reference only, never lifted.** Nothing in this repository is copied from any of
the projects below. Where prior work made something understandable, it is credited
here and the implementation is written independently - see
[CLAUDE.md](CLAUDE.md) principle 1 and [docs/DECISIONS.md](docs/DECISIONS.md) D024.

If you consult something while working on this, add it here in the same commit. An
entry costs a line. An uncredited influence is the thing that makes a provenance
question unanswerable later.

Note the distinction that matters: reading another project's **prose, design notes,
or public documentation** to understand a format is ordinary engineering. Reading
its source and reproducing the structure is not, and neither is reading vendor
binaries. The line is D014, not the presence of an entry here.

## Standards and open-source references

| Reference | What it informed |
|-----------|------------------|
| **FreeBSD** | The strongest lawful reference available for the guest kernel surface. Much of it is POSIX with vendor naming, so the documented BSD semantics are the specification wherever an analogue exists. Cite per function when implementing. |
| **The ELF specification** | Container parsing. The standard header and program-table handling in `orbistoun-elf` follows it directly. |
| **Vulkan / SPIR-V (Khronos)** | The GPU translation target. |
| **[OpenOrbis Toolchain](https://github.com/OpenOrbis/OpenOrbis-PS4-Toolchain)** | A legal, open-source LLVM/Clang cross-compiler built without vendor tools. Used to build our own test material, and read as a lawful source of **interface facts** - symbol names and argument counts. Never as a source of implementations; see D044 for the line. |
| **FreeBSD headers** | 911 ABI constants - error numbers, signal numbers, address families, socket options, open flags, kernel MIB identifiers - harvested from six public headers into `abi-constants.toml`. Interface facts, not implementation: a `#define` is the ABI. Read rather than retyped, and recorded as FreeBSD-published and target-assumed, because the target is FreeBSD-*derived* (D351). |
| **The GNU hash ELF extension** | A public extension to ELF, documented openly. `symbol_count_from_gnu_hash` in `orbistoun-elf` implements the bucket-and-chain walk it describes, which is the only way to recover a symbol count from a table that states none (D305). |
| **GNU binutils (`readelf`)** | Used as an **independent oracle**, not as a reference. Import counts this project derives are cross-checked against what `readelf` reports for the same file - it agreed on all five payloads exactly, and caught a first pass that was one too high on every row (D305). A number a tool produces about itself is not a measurement until something else says it too. |
| **`zerocopy`, `ash`, `clap`, `serde`, `tracing`, and the rest of the dependency tree** | See `Cargo.toml`. `deny.toml` holds a permissive-only allow list, and three transitive GUI dependencies currently fall outside it - see the licence entry in [docs/BACKLOG.md](docs/BACKLOG.md). |

## GPU hardware documentation

**AMD instruction set architecture reference guides.** The hardware use AMD graphics
hardware, and AMD publishes the full instruction set openly - one reference guide per
architecture generation. Those documents are the source for the encoding table in
`crates/orbistoun-shader/data/encodings.toml`.

**The command-processor packet format**, likewise publicly documented, and parsed by
the open-source Linux kernel driver and by Mesa - both of which are open-source
software for publicly documented hardware.

Worth being explicit about why this sits comfortably inside rule 1. This is hardware
documentation from the *chip* vendor, not firmware from the *hardware* vendor. It
describes a silicon interface that AMD ships in retail parts and documents for anyone
writing a driver. It is the same category as the FreeBSD source in the oracle list -
lawful, citable, and by a distance the best reference material available anywhere in
this project.

The hardware parts are customised, so the published tables cover most of what appears
in a real shader and not all of it. Anything they do not describe is counted and
reported as unknown rather than guessed at.

## Sibling projects, same author

**obSCEne** and **Prosperous** are this project's own siblings rather than third-party work,
so nothing here is owed to anyone else - but they are listed because "where did that come
from" should always have an answer, and because a reader finding a fact here that originated
next door deserves the pointer.

- **obSCEne** - a conformance probe for the same platform. Its `docs/` answered the vendor
  dynamic tags before an afternoon here rediscovered them. Read read-only, and it is also
  the control that shows a data-import problem is a C++ runtime problem: a freestanding
  probe imports none and has zero of them, where every commercial title has five to twenty
  (D307).
- **Prosperous** - one instrument for talking to a jailbroken hardware, and the library under
  it. Its `pros check` names exactly the five services this work is aimed at, which is where
  the target came from.
- **An earlier project of my own** - not part of this collection, and not public. It reached
  an installed coding assistant by running it as a subprocess, which sidesteps needing an API
  key, and
  `crates/orbistoun-llm/src/cli.rs` does the same thing here. Its **binary-discovery order on
  Windows is carried across along with the reasoning for it**: the launcher under
  `LOCALAPPDATA` hands off to a running desktop application and the caller never sees the
  reply, so the versioned command under `APPDATA` is preferred and the launcher is a last
  resort. That is a fact about the platform somebody had to find by being caught by it, and
  it is cited rather than rediscovered.

  Two things were deliberately **not** carried over, and the reasons are in D333: the prompt
  goes on standard input rather than in the argument list, and a signed-out command is
  reported rather than signed in on the caller's behalf.

## Open-source homebrew read as guest material

**[ps5-payload-dev](https://github.com/ps5-payload-dev) (John Törnblom and contributors,
GPL-3.0).** `elfldr`, `ftpsrv`, `klogsrv`, `shsrv`, `pldmgr` and the rest of that family are
**guests** here, in the same sense a title is: orbistoun loads them and reports what they
ask for. What was read is what a loader reads - ELF headers, program headers, the dynamic
table, the import list, and the symbol table - plus eight bytes at one trap site, to tell a
deliberate `ud2` from a guest derailed into data (D305, D306, D308).

Two lines are held deliberately:

- **Nothing is copied.** They are GPL-3.0 and this project is MIT/Apache-2.0, so lifting
  even a struct declaration would be a licensing problem quite apart from the provenance
  one. Where their published documentation describes an interface, it is read as prose and
  written out independently, recorded `published`, and promoted to `measured` only when a
  run agrees.
- **No payload binary is tracked here.** They live outside the repository, like every other
  guest, and the provenance job fails the build on any of it.

Their existence is also what made D163 answerable at all: a family of open, freely
redistributable guests that exercise a real import table is the test material this project
otherwise has to build for itself.

**PS5SDK (`PS5Dev/PS5SDK`) - seen, not used.** A search for the payload calling convention
returned a summary of this project's argument structure. It is a **different SDK** from the
one the payloads here are built with - that one uses dynamic linking, which is why those
payloads carry real import tables at all - and nothing in this repository is derived from
it. Recorded because an uncredited sighting is what makes a provenance question
unanswerable later, and because the temptation to use it was real: its first field is a
function pointer, and the measurement here says the entry point calls its first field
immediately (D308). That agreement is suggestive and is **not** evidence about the SDK
actually in use.

## Prior art in this space

Other emulation projects established that high-level emulation of this target is
tractable at all, and their **public writing** - design notes, issue discussions,
architecture descriptions - informed the shape of this one. Their source was not
used as a basis for any code here.

The specific things learned from watching the field, all of which shaped the roadmap:

- High-level emulation makes incremental progress; hypervisor approaches to the same
  target have not reached a running title. Recorded as reasoning in
  [docs/SCOPE.md](docs/SCOPE.md).
- Hardware-probe-driven development - write a test program, run it on real hardware,
  encode the observed behaviour as a test - is the strongest per-function oracle
  anyone in this space has found. In [docs/BACKLOG.md](docs/BACKLOG.md).
- Progress is better measured by unresolved-import counts than by screenshots.

*(Named entries to be added as specific references are actually consulted, rather
than pre-populated with projects nobody here has read.)*

## Symbol names

Symbol and library names are interface identifiers - facts about an ABI. Where this
project resolves them, it does so by hashing candidate names and matching the result
against hashes observed in a module's own import table (D025). That derivation is
self-verifying and requires reading no vendor binary.

No symbol database is distributed with this repository.
