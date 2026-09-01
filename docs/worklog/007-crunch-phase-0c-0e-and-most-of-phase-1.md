# 2026-08-19 - Crunch: phase 0c, 0e, and most of phase 1


First implementation session. **118 tests passing, clippy clean at `-D warnings`.**
Went from 22 tests to 118, and from 14 crates to 20.

**Phase 0c - structural seams, complete.**
- Bin target renamed `orbistoun` → `orbistoun-cli` (D037), workflow and script updated.
- **`orbistoun-gpu` split.** The translator now has *no* `ash` dependency;
  `orbistoun-gpu-vulkan` is the only crate naming a graphics API. `RecordingBackend`
  makes translation testable with no GPU - the thing that justifies the seam existing
  now rather than later.
- **`orbistoun-paths`** (13 tests) with the containment test D038 asked for: write
  through every location the API exposes, assert nothing landed outside the root.
- **`orbistoun-overrides`** (11 tests), including a test named for the failure the
  whole design exists to prevent - a user setting must not drop repo compatibility
  entries.
- **`orbistoun-proto`** (12 tests) with framing in a separable module.
- **`orbistoun-service`** (11 tests). The CLI now holds no logic and depends on the
  service alone.

**Phase 0e - observability substrate.** `orbistoun-report` (27 tests): versioned
bounded schema, `RunDiff`, a store that answers "what did the previous run of this
title do?", and retention with both an age and a byte guard.

**Phase 1 - wrapper parsing works against real material.** See D052. The container
layer now parses all four real containers, and `orbistoun-cli inspect` reports their
structure.

**Surprises, all from real material rather than reasoning.**
- **The ELF program headers are not file offsets.** Eleven of fourteen point past
  end-of-file. The wrapper's descriptor table is the real map - flags carry the target
  header index in bits 20+, and the `0x800` bit marks data-bearing descriptors. Fully
  decoded and verified; the Rust implementation independently reproduces the mapping
  the analysis found.
- **Two of my own assertions were wrong**, and only real files showed it. The vendor
  `p_type` range was too narrow (saw one of three vendor segments). And the header's
  size field is *not* the file length - I had implemented it as a truncation check and
  it was reporting a false "MISMATCH" on every valid file. Both corrected in D052.
- **A malformed-input test found a real panic.** A descriptor claiming an enormous
  size overflowed the range arithmetic. Parsers see hostile input by definition; it
  saturates now.
- **Writing the test first would have caught two wrong expectations faster.** Two
  tests failed on *my* assertions rather than the code - the metadata blocks' index
  bits mean something other than a program-header index. That is now documented on the
  accessor.

**Phase 1 finished in the same session.** See D053. The import path turned out to be
**standard ELF machinery** - `PT_DYNAMIC`, `DT_STRTAB`, `DT_SYMTAB`, `DT_HASH`, an
ordinary symbol table - with only the *names* vendor-encoded (`H2e8t5ScQGc#B#C` is a
base64 NID plus library and module ids). Far less bespoke than the vendor `DT_` tags
suggested; the tags are a red herring and get skipped.

`orbistoun-cli imports` now works on real material: **159** imports from a 76 KB
module, 117 and 95 from two others, **1,410 from the commercial executable**, each
attributed to a library via `DT_NEEDED`.

Consequence worth noting: an import's NID is *in its name*, so surveying needs no hash
suffix and no symbol database at all. D006's suffix is only needed to hash our own
declarations in the other direction.

**Two limitations left honest rather than papered over:** the decoded NID's byte order
is self-consistent but unverified against any known (name, hash) pair; and 70 of the
1,410 imports have no library attributed, because their library id does not index
`DT_NEEDED` - probably a separate table in the vendor `DT_` entries. Reported as `?`.

**One more surprise, from the gate rather than the code.** `cargo-deny` rejected
`option-ext` (MPL-2.0), reached via `directories` -> `dirs-sys`. D003's allow-list was
written permissive-only, but the concern it actually records is *binary relicensing* -
and MPL-2.0 is file-level copyleft, which does not reach our code. Allowed with that
distinction stated; GPL and AGPL stay excluded. Considered and rejected hand-rolling
the OS data-dir logic to avoid the dependency: thirty lines, but a worse end state
under D028 for a concern that does not apply.

Also pruned `ash` from `orbistoun-gpu-vulkan` - the backend is a stub that uses no
Vulkan yet, so the dependency was aspirational (D019). It returns with the device. The
boundary is enforced by `orbistoun-gpu` having no path to a graphics API, not by the
backend crate having one.

**Next.** Phase 2 - symbol database loading, to turn 1,410 unresolved hashes into
names. That is now the highest-value item: the unresolved count is real, and naming
them is what makes it a work list.

