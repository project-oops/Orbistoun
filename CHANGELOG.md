# Changelog

orbistoun ships as a **single rolling build** - no tagged versions, no semantic
versioning. Every push to `main` rebuilds one `latest-main` artifact, so the
**short commit SHA is the version number**.

Each entry below is headed by the SHA (+ date) that shipped it, newest first.
Within an entry, changes are grouped **Added / Changed / Fixed**.

Nothing has shipped yet. This is the initial commit, so no entry below carries a SHA and none
of the CI that would produce one has ever run.

## [unreleased] - as of 2026-09-01

### Added

- **A Cargo workspace** on edition 2024, structured as a dependency spine
  (`core` -> `elf` -> `nid` -> `mem` -> `hle` -> `loader`) with execution,
  guest-OS, graphics, tooling and shell crates above it.
- **Container parsing.** ELF64 headers, the vendor wrapper, the dynamic table, imports,
  and relocations - with zero `unsafe`, via `zerocopy`. Verified against real material.
- **The loader.** Parse, reserve, place, resolve, relocate, TLS, entry point. Every
  commercial executable in the local corpus goes from bytes to a running guest.
- **Execution.** Guest x86-64 runs natively in an isolated worker process, with per-import
  thunks catching every call out. A fault happens in the child; the parent survives to
  write out what was learned.
- **`guest_module!` macro + HLE registry.** A subsystem declares its library and functions
  in one block; the registry resolves NID to declaration. Stub behaviour is a runtime TOML
  file keyed by symbol name, so bisecting a function's semantics costs a relaunch instead
  of a rebuild.
- **69 implemented system functions** across libc, libkernel, the filesystem, video output
  and system services - chosen by measurement rather than by which subsystem looked
  interesting.
- **NID hashing** with the hash suffix as runtime data rather than a source constant.
- **The name search.** A candidate grammar, threaded indexable enumeration, a
  standard-library word list harvested from FreeBSD source, and identifier-shaped strings
  read out of guest modules' own bytes - pooled across the whole corpus, so one title's
  data names another title's imports. **737 names**, every one confirmed by the hash;
  520 re-derivable from this repository alone and nothing unaccounted for.
- **Provenance vocabulary with two axes.** Every name records what kind of material
  proposed it - `derived`, `static`, `runtime`, `external` - and what somebody else would
  need to arrive at it again. `audit --verify-harvest` re-reads the module behind every
  static record; `audit --repair` re-derives records whose coordinates the grammar moved
  underneath.
- **Run reports.** A persisted call trace per module, a FURTHER/same/BACK progress
  verdict against the previous run, the conditions a run was made under, fault detail with
  registers and call path, argument dumps, and a ranked list of findings with actions.
  A dumped argument says which of three things it is - a scalar, mapped bytes, or an
  address pointing at nothing this run mapped - because the first and last read identically
  as a bare number and mean opposite things (D217).
- **One definition of an open question.** The count is the length of the list a report
  prints, so the summary and the queue cannot disagree - they did, by ten, in four separate
  copies of the counting rule (D239). A `cites` value naming a filesystem path is refused by
  the provenance audit: a citation must name a document, not a location on one machine.
- **The knowledge base.** Every recorded behaviour carries `known_by` - `published`,
  `measured`, `guest-observed` or `assumed` - with citations or stated assumptions, and
  `orbistoun-cli questions` ranks every open question by how often a guest calls the
  function.
- **Six run diagnostics**, each answering one question and each recorded in the run's
  conditions so a verdict taken under one is never compared with an ordinary one: forced
  argument dumps, a poisoned guest stack, a poisoned heap, a value planted at an argument's
  target, and self-identifying values written into the memory-query structure. Every one
  addresses an import by name *or by hash*, so an unnamed function can be experimented on
  (D185, D198, D218, D220). A plant reaches any member of a structure, not only the word an
  argument points at, and several at once - so eight candidate slots are one run rather than
  eight (D229). A sixth forces what an import *answers*, in full 64 bits, which the
  name-keyed 32-bit policy file could express for no unnamed function at all (D230).
- **Every diagnostic reports what it actually did** - plants landed and refused, calls
  answered - because a diagnostic that never ran and one that ran and changed nothing
  produce identical output, and two recorded eliminations turned out to be the first kind
  (D229, D230).
- **All sixteen registers at a fault**, which were captured and written to the trace from
  the start and dropped by the renderer, which printed four (D230).
- **One registry for every environment variable** (`orbistoun-env`) - name, purpose,
  example, and which crate reads it. `orbistoun-cli env` prints it and flags anything set
  that is *nearly* one of them, because a misspelled variable is an absence rather than an
  error and otherwise produces an ordinary result that gets believed. Settings and
  diagnostics are distinguished, which is what decides that a future `.env` may carry the
  first and never the second (D221).
- **A configurable physical memory map** (`whole`, `reserved-low`, `fragmented`), so the
  shape a guest walks is a variable rather than an assumption. Every shape is tested to
  cover the whole range without gaps.
- **Every build says which build it is.** The commit appears in the GUI sidebar and in
  `orbistoun-cli paths`, marked `-dirty` when built from uncommitted edits, falling back to
  the compile time where there is no commit. CI supplies the SHA and a build script asks
  git otherwise, so a plain clone stamps itself with no configuration. This also populated
  `binary_commit` in run reports, which had read `"unknown"` since the field was written
  because nothing ever set it (D222).
- **A deterministic guest limit.** A call budget fixes the number of imports a run may
  call, so two runs of one build stop at the same call and a verdict between them measures
  the change rather than the machine - three runs of the title that varied 13% now return
  20,000,000 calls each. The wall clock stays as a backstop for a guest that stops calling
  imports, and the exit status says which of the two fired (D238).
- **Per-title compatibility records**, written from a trace rather than by hand.
- **The GPU layer, ahead of the spine.** Command-packet walking, register file, pipeline
  assembly, shader decoding and an instruction census, SPIR-V construction, and shader
  translation checked by executing the result on a real device rather than validating its
  structure.
- **The vendor's own dynamic tables are read.** A hardware loader ignores `DT_STRTAB`,
  `DT_SYMTAB` and `DT_HASH` and reads `DT_SCE_*` entries pointing into a
  `PT_SCE_DYNLIBDATA` segment, at offsets rather than virtual addresses. orbistoun read only
  the standard tags and worked solely because every title in the corpus carries both sets;
  a module built the way the platform expects was refused outright (D247).
- **The conformance probe runs.** obSCEne's minimal module loads, links, resolves its import
  and prints from inside the guest through orbistoun's `sceKernelWrite` - the first guest
  here that is not a commercial title, and the first whose imports arrive already named.
- **A conformance-probe reader**, built and tested against captured transcripts with no
  hardware attached. It reads the probe's by-name symbol census, which it previously carried
  as an unrecognised record and drew no fact from (D245), and grades an existence claim by
  what it ran on - only the target may source a name (D242, D246).
- **`orbistoun-cli`** with twenty commands, and **`orbistoun-gui`**, both shims over
  `orbistoun-service`.
- **Window capture from the GUI toolbar**, written to `<data>/screenshots/`. Deliberately
  named *capture* rather than *screenshot*: there is no guest frame yet, and the panels it
  does take - a call tail, a register dump, a ranked finding list - are worth having on
  their own. Recording sits beside it disabled, with the reason on hover (D215).
- **Provenance guard in CI** - fails the build on committed firmware, keys, dumps, or
  guest binaries. Also runs unconditionally in the pre-push hook.
- **Workspace lints table** with the unsafe-discipline lints at `deny`
  (`undocumented_unsafe_blocks`, `multiple_unsafe_ops_per_block`,
  `unsafe_op_in_unsafe_fn`), so rust-analyzer enforces them while you type rather
  than only at CI.
- **Rustdoc published to Pages** alongside the landing page, at `/doc/`.
- **Language-model access** (`orbistoun-llm`) - measures the machine, sizes a model
  catalogue to it, writes an ordered backend registry, and downloads a model on first
  use. Local first: the seeded ladder runs on this machine before it reaches anything
  hosted. Deterministic by default, and a reply says which model produced it. Depends on
  no other crate here, so the callers that will close the loop's two manual steps can
  each be shaped for their own job (D212).
- **Proposals paired with an oracle** (`orbistoun-propose`). The first one asks a model
  for candidate *words* - never for a name - grows the grammar in memory, and keeps only
  what the NID hash confirms. A name found this way records as `generated` at a pattern
  and index, so `audit` re-derives it like any other; a wrong suggestion costs nothing.
  Sweeps only the shapes the new words reach, which is 83x cheaper and, unlike the
  narrowing past it, leaves the record intact (D214).

### Not yet

No pixel has been rendered, no guest has spawned a thread, and no title reaches its own
main loop. Three walls, all in guest startup, are named in
[docs/PROJECT_STATUS.md](docs/PROJECT_STATUS.md).
