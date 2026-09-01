# Decision log

Append-only. Every decision that shaped this codebase, with its reasoning, so it can
survive a context compaction and so nobody re-litigates a settled question from
first principles six weeks later.

**Read this file at the start of any working session.** It is the durable memory of
the project; the conversation that produced these decisions is not.

**This table is generated.** Edit an entry under `decisions/`, then run
`tools/split-decisions.sh --index orbistoun`. A number resolves to exactly one file.

| | # | decision | status | date |
|---|---|---|---|---|
| 🟢 | D001 | [Rust, not C++ or C#](decisions/D001-rust-not-c-or-c.md) | decided | 2026-08-19 |
| 🟢 | D002 | [Edition 2024, MSRV 1.85, toolchain pinned to 1.97.1](decisions/D002-edition-2024-msrv-1-85-toolchain-pinned.md) | decided | 2026-08-19 |
| 🟢 | D003 | [MIT OR Apache-2.0; permissive-only dependency policy](decisions/D003-mit-or-apache-2-0-permissive-only.md) | decided | 2026-08-19 |
| 🟢 | D004 | [Fourteen crates arranged as a dependency spine](decisions/D004-fourteen-crates-arranged-as-a.md) | decided | 2026-08-19 |
| 🟢 | D005 | [Interception is linking, not hooking](decisions/D005-interception-is-linking-not-hooking.md) | decided | 2026-08-19 |
| 🟢 | D006 | [The NID hash suffix is runtime data, not a source constant](decisions/D006-the-nid-hash-suffix-is-runtime-data-not.md) | decided | 2026-08-19 |
| 🟢 | D007 | [Symbol databases store names; NIDs are derived](decisions/D007-symbol-databases-store-names-nids-are.md) | derived | 2026-08-19 |
| 🟢 | D008 | [Stub policy defaults to `Unimplemented`, never `Ok`](decisions/D008-stub-policy-defaults-to-unimplemented.md) | decided | 2026-08-19 |
| 🟢 | D009 | [Placeholder error codes avoid the high bit](decisions/D009-placeholder-error-codes-avoid-the-high.md) | decided | 2026-08-19 |
| 🟢 | D010 | [Honest failure over plausible output](decisions/D010-honest-failure-over-plausible-output.md) | decided | 2026-08-19 |
| 🟢 | D011 | [The container parser contains zero `unsafe`](decisions/D011-the-container-parser-contains-zero.md) | decided | 2026-08-19 |
| 🟢 | D012 | [Vendor `p_type` range asserted; individual constants are not](decisions/D012-vendor-p-type-range-asserted-individual.md) | decided | 2026-08-19 |
| 🟢 | D013 | [Lints live in a workspace table, with unsafe discipline at deny](decisions/D013-lints-live-in-a-workspace-table-with.md) | decided | 2026-08-19 |
| 🟢 | D014 | [Provenance is enforced in CI and in the pre-push hook](decisions/D014-provenance-is-enforced-in-ci-and-in-the.md) | decided | 2026-08-19 |
| 🟢 | D015 | [No vendor trademarks in prose or in our own API](decisions/D015-no-vendor-trademarks-in-prose-or-in-our.md) | decided | 2026-08-19 |
| 🟢 | D016 | [Validation is separated from effects](decisions/D016-validation-is-separated-from-effects.md) | decided | 2026-08-19 |
| 🟢 | D017 | [Handles are per-subsystem and never recycled](decisions/D017-handles-are-per-subsystem-and-never.md) | decided | 2026-08-19 |
| 🟢 | D018 | [Traces are binary, sequence-numbered, and call-site attributed](decisions/D018-traces-are-binary-sequence-numbered-and.md) | decided | 2026-08-19 |
| 🟢 | D019 | [Greenfield: no legacy, no compatibility shims](decisions/D019-greenfield-no-legacy-no-compatibility.md) | decided | 2026-08-19 |
| 🟢 | D020 | [Synthetic fixtures are a prerequisite, not a nice-to-have](decisions/D020-synthetic-fixtures-are-a-prerequisite.md) | decided | 2026-08-19 |
| 🟢 | D021 | [The ABI spike happens out of order](decisions/D021-the-abi-spike-happens-out-of-order.md) | decided | 2026-08-19 |
| 🟢 | D022 | [The trace sink is wired at phase 4, not earlier](decisions/D022-the-trace-sink-is-wired-at-phase-4-not.md) | decided | 2026-08-19 |
| 🟢 | D023 | [Release profile favours speed; debug info kept](decisions/D023-release-profile-favours-speed-debug.md) | decided | 2026-08-19 |
| 🟢 | D024 | [Other projects are reference-only, and get credited](decisions/D024-other-projects-are-reference-only-and.md) | decided | 2026-08-19 |
| 🟢 | D025 | [Generate the symbol name list; never ship a database](decisions/D025-generate-the-symbol-name-list-never.md) | decided | 2026-08-19 |
| 🟢 | D026 | [Crunch until the queue is dry, then re-plan](decisions/D026-crunch-until-the-queue-is-dry-then-re.md) | decided | 2026-08-19 |
| 🟢 | D027 | [Linux verification via multipass; Windows is the primary host](decisions/D027-linux-verification-via-multipass.md) | decided | 2026-08-19 |
| 🟢 | D028 | [No urgency; take the highest-payoff path](decisions/D028-no-urgency-take-the-highest-payoff-path.md) | decided | 2026-08-19 |
| 🟢 | D029 | [Contracts and swappable backends, abstracted at guest semantics](decisions/D029-contracts-and-swappable-backends.md) | decided | 2026-08-19 |
| 🟢 | D030 | [Backend seams are enforced by crate boundaries, not discipline](decisions/D030-backend-seams-are-enforced-by-crate.md) | decided | 2026-08-19 |
| 🟢 | D031 | [Where the abstraction principle deliberately stops](decisions/D031-where-the-abstraction-principle.md) | decided | 2026-08-19 |
| 🟢 | D032 | [The guest executes in a child process](decisions/D032-the-guest-executes-in-a-child-process.md) | decided | 2026-08-19 |
| 🟢 | D033 | [Worker mode is self-reinvocation; no binary is privileged](decisions/D033-worker-mode-is-self-reinvocation-no.md) | decided | 2026-08-19 |
| 🟢 | D034 | [CLI, GUI, and worker are interaction shims; logic lives in the crates](decisions/D034-cli-gui-and-worker-are-interaction.md) | decided | 2026-08-19 |
| 🟢 | D035 | [The protocol is serialisable data, defined separately from its transport](decisions/D035-the-protocol-is-serialisable-data.md) | decided | 2026-08-19 |
| 🟢 | D036 | [GUI: egui, not Tauri](decisions/D036-gui-egui-not-tauri.md) | decided | 2026-08-19 |
| 🟢 | D037 | [Two user-facing binaries, plus a portable GUI artifact](decisions/D037-two-user-facing-binaries-plus-a.md) | decided | 2026-08-19 |
| 🟢 | D038 | [Portable mode specification](decisions/D038-portable-mode-specification.md) | decided | 2026-08-19 |
| 🟢 | D039 | [Configuration formats: TOML and JSON; no database](decisions/D039-configuration-formats-toml-and-json-no.md) | decided | 2026-08-19 |
| 🟢 | D040 | [GUI is library-first; Inspect before Launch](decisions/D040-gui-is-library-first-inspect-before.md) | decided | 2026-08-19 |
| 🟢 | D041 | [Firmware / system-menu launch is the lowest-priority stretch goal](decisions/D041-firmware-system-menu-launch-is-the.md) | decided | 2026-08-19 |
| 🟢 | D042 | [A manifest-driven test corpus under `titles/`](decisions/D042-a-manifest-driven-test-corpus-under.md) | decided | 2026-08-19 |
| 🟢 | D043 | [The accuracy suite is a separate repository, `obSCEne`](decisions/D043-the-accuracy-suite-is-a-separate.md) | decided | 2026-08-19 |
| 🟢 | D044 | [The open toolchain is a lawful source for interface facts, never for implementations](decisions/D044-the-open-toolchain-is-a-lawful-source.md) | decided | 2026-08-19 |
| 🟢 | D045 | [obSCEne result model, ordering, and coverage generation](decisions/D045-obscene-result-model-ordering-and.md) | decided | 2026-08-19 |
| 🟢 | D046 | [Three observability channels, not one](decisions/D046-three-observability-channels-not-one.md) | decided | 2026-08-19 |
| 🟢 | D047 | [Files first; OTLP export is an opt-in feature, not the substrate](decisions/D047-files-first-otlp-export-is-an-opt-in.md) | decided | 2026-08-19 |
| 🟢 | D048 | [Per-title overrides: one file per title, three layers, merged per key](decisions/D048-per-title-overrides-one-file-per-title.md) | decided | 2026-08-19 |
| 🟢 | D049 | [Containers are wrapped; the ELF is not at offset zero](decisions/D049-containers-are-wrapped-the-elf-is-not.md) | decided | 2026-08-19 |
| 🟢 | D050 | [Revised starting order after real material arrived](decisions/D050-revised-starting-order-after-real.md) | decided | 2026-08-19 |
| 🟢 | D051 | [Fixtures are generated, never extracted](decisions/D051-fixtures-are-generated-never-extracted.md) | decided | 2026-08-19 |
| 🟢 | D052 | [The wrapper descriptor table locates segment data, not the ELF headers](decisions/D052-the-wrapper-descriptor-table-locates.md) | decided | 2026-08-19 |
| 🟢 | D053 | [Imports use standard ELF machinery; only the names are vendor-encoded](decisions/D053-imports-use-standard-elf-machinery-only.md) | decided | 2026-08-19 |
| 🟢 | D054 | [A module is reserved as one contiguous span, not per segment](decisions/D054-a-module-is-reserved-as-one-contiguous.md) | decided | 2026-08-19 |
| 🟢 | D054 | [Distribution formats stop at the door; mounts are ordered](decisions/D054-distribution-formats-stop-at-the-door.md) | decided | 2026-08-19 |
| 🟢 | D055 | [The Linux path was broken, and only running it showed that](decisions/D055-the-linux-path-was-broken-and-only.md) | decided | 2026-08-19 |
| 🟢 | D055 | [Title metadata is cheap, not a second container format](decisions/D055-title-metadata-is-cheap-not-a-second.md) | decided | 2026-08-19 |
| 🟢 | D056 | [A remote hardware probe is the top-value future capability](decisions/D056-a-remote-hardware-probe-is-the-top.md) | hardware | 2026-08-19 |
| 🟢 | D056 | [The guest-to-host call boundary works, and needs `extern "sysv64"`](decisions/D056-the-guest-to-host-call-boundary-works.md) | decided | 2026-08-19 |
| 🟢 | D057 | [Worker mode is implemented, and both shims go through it](decisions/D057-worker-mode-is-implemented-and-both.md) | decided | 2026-08-19 |
| 🟢 | D058 | [Image placement, and the host allocation granularity](decisions/D058-image-placement-and-the-host-allocation.md) | decided | 2026-08-19 |
| 🟢 | D059 | [Relocations apply; "interception is linking" is now machine code](decisions/D059-relocations-apply-interception-is.md) | decided | 2026-08-19 |
| 🟢 | D060 | [Protection is computed per page, as a union](decisions/D060-protection-is-computed-per-page-as-a.md) | decided | 2026-08-19 |
| 🟢 | D061 | [Thread-local storage is variant II, and the block sits *below* the pointer](decisions/D061-thread-local-storage-is-variant-ii-and.md) | decided | 2026-08-19 |
| 🟢 | D062 | [One stub per import, not one shared target](decisions/D062-one-stub-per-import-not-one-shared.md) | decided | 2026-08-19 |
| 🟢 | D063 | [Guest code runs on its own stack, entered through a switch](decisions/D063-guest-code-runs-on-its-own-stack.md) | decided | 2026-08-19 |
| 🟢 | D064 | [A worker that dies without a verdict gets a postmortem](decisions/D064-a-worker-that-dies-without-a-verdict.md) | decided | 2026-08-19 |
| 🟢 | D065 | [Execute is never dropped, and never means "no access"](decisions/D065-execute-is-never-dropped-and-never.md) | decided | 2026-08-19 |
| 🟢 | D066 | [A guest that will not stop is a result, not a hang](decisions/D066-a-guest-that-will-not-stop-is-a-result.md) | decided | 2026-08-19 |
| 🟢 | D067 | [A stub index is worthless; a name is a work list](decisions/D067-a-stub-index-is-worthless-a-name-is-a.md) | decided | 2026-08-19 |
| 🟢 | D068 | [Names are generated and proved, never obtained](decisions/D068-names-are-generated-and-proved-never.md) | decided | 2026-08-19 |
| 🟢 | D069 | [Nothing is searched without a suffix, and a miss on standard names indicts it](decisions/D069-nothing-is-searched-without-a-suffix.md) | decided | 2026-08-19 |
| 🟢 | D070 | [The hasher read the digest backwards, from the very first commit](decisions/D070-the-hasher-read-the-digest-backwards.md) | decided | 2026-08-19 |
| 🟢 | D071 | [The hash suffix ships with the repository](decisions/D071-the-hash-suffix-ships-with-the.md) | decided | 2026-08-19 |
| 🟢 | D072 | [First names, and the first concrete implementation target](decisions/D072-first-names-and-the-first-concrete.md) | decided | 2026-08-19 |
| 🟢 | D073 | [Every name records how it was found, and the record is checked](decisions/D073-every-name-records-how-it-was-found-and.md) | decided | 2026-08-19 |
| 🟢 | D074 | [Symbol databases accumulate, and are generated by a task](decisions/D074-symbol-databases-accumulate-and-are.md) | decided | 2026-08-19 |
| 🟢 | D075 | [Standard-library names are harvested from FreeBSD, not remembered](decisions/D075-standard-library-names-are-harvested.md) | decided | 2026-08-19 |
| 🟢 | D076 | [Names are ours; selection is the module's](decisions/D076-names-are-ours-selection-is-the-module-s.md) | decided | 2026-08-19 |
| 🟢 | D077 | [The loop is a verb, and its output is durable](decisions/D077-the-loop-is-a-verb-and-its-output-is.md) | decided | 2026-08-19 |
| 🟢 | D078 | [`sweep`, not `crunch`](decisions/D078-sweep-not-crunch.md) | decided | 2026-08-19 |
| 🟢 | D079 | [`run <title>` does everything a run needs](decisions/D079-run-title-does-everything-a-run-needs.md) | decided | 2026-08-19 |
| 🟢 | D080 | [The loop has an objective function](decisions/D080-the-loop-has-an-objective-function.md) | decided | 2026-08-19 |
| 🟢 | D081 | [`run` checks the machine before it tries anything](decisions/D081-run-checks-the-machine-before-it-tries.md) | decided | 2026-08-19 |
| 🟢 | D082 | [Declarations reach the guest, and the first function is implemented](decisions/D082-declarations-reach-the-guest-and-the.md) | decided | 2026-08-19 |
| 🟢 | D083 | [The guest ignores return codes; the buffer is the channel](decisions/D083-the-guest-ignores-return-codes-the.md) | decided | 2026-08-19 |
| 🟢 | D084 | [A tech-debt pass, and the vocabulary grows from our own observations](decisions/D084-a-tech-debt-pass-and-the-vocabulary.md) | decided | 2026-08-19 |
| 🟡 | D084 | [Instrument the GPU before translating any of it](decisions/D084-instrument-the-gpu-before-translating.md) | assumed | 2026-08-19 |
| 🟢 | D085 | [The hash suffix is `supplied`, and the file now says so](decisions/D085-the-hash-suffix-is-supplied-and-the.md) | decided | 2026-08-19 |
| 🟡 | D085 | [The instruction encoding table is data, and says it is unverified](decisions/D085-the-instruction-encoding-table-is-data.md) | assumed | 2026-08-19 |
| 🔴 | D086 | [Rank blockers by shaders blocked, within an effort tier](decisions/D086-rank-blockers-by-shaders-blocked-within.md) | blocked | 2026-08-19 |
| 🟡 | D087 | [Decoding never fails; it reports](decisions/D087-decoding-never-fails-it-reports.md) | assumed | 2026-08-19 |
| 🟡 | D088 | [The shader corpus is content-addressed, and is the regression suite](decisions/D088-the-shader-corpus-is-content-addressed.md) | assumed | 2026-08-19 |
| 🟢 | D089 | [Verify the encoding table against a reference disassembler](decisions/D089-verify-the-encoding-table-against-a.md) | decided | 2026-08-19 |
| 🟡 | D090 | [Instruction names come only from what was observed](decisions/D090-instruction-names-come-only-from-what.md) | assumed | 2026-08-19 |
| 🟡 | D091 | [Register extraction is mechanism; the shader-address map is a hypothesis](decisions/D091-register-extraction-is-mechanism-the.md) | assumed | 2026-08-19 |
| 🟡 | D092 | [Report rendering lives in the library](decisions/D092-report-rendering-lives-in-the-library.md) | assumed | 2026-08-19 |
| 🟢 | D093 | [Operands are decoded, and the numbering lives in data](decisions/D093-operands-are-decoded-and-the-numbering.md) | decided | 2026-08-19 |
| 🟢 | D094 | [Scalar destinations use the shared operand numbering](decisions/D094-scalar-destinations-use-the-shared.md) | decided | 2026-08-19 |
| 🟡 | D095 | [The shader census has a CLI surface](decisions/D095-the-shader-census-has-a-cli-surface.md) | assumed | 2026-08-19 |
| 🟢 | D096 | [Operand layout is a property of the opcode, not of the encoding family](decisions/D096-operand-layout-is-a-property-of-the.md) | decided | 2026-08-19 |
| 🟢 | D097 | [Per-opcode operand fields are solved from probes, not transcribed](decisions/D097-per-opcode-operand-fields-are-solved.md) | decided | 2026-08-19 |
| 🟢 | D098 | [Predicated translation first; structured reconstruction stubbed loudly](decisions/D098-predicated-translation-first-structured.md) | decided | 2026-08-19 |
| 🟢 | D099 | [Translated shaders are executed, not merely validated](decisions/D099-translated-shaders-are-executed-not.md) | decided | 2026-08-19 |
| 🟢 | D100 | [Three wavefront models, kept as a differential oracle](decisions/D100-three-wavefront-models-kept-as-a.md) | decided | 2026-08-20 |
| 🟡 | D101 | [Guest memory is a second binding, never a second half of the first](decisions/D101-guest-memory-is-a-second-binding-never.md) | assumed | ~2026-08-20..08-21 |
| 🟢 | D102 | [The SPIR-V builder holds the section layout, not its callers](decisions/D102-the-spir-v-builder-holds-the-section.md) | decided | 2026-08-21 |
| 🟢 | D103 | [The builder checks that its identifiers resolve](decisions/D103-the-builder-checks-that-its-identifiers.md) | decided | 2026-08-21 |
| 🔴 | D104 | [A blocked instruction names its dependency; an unwritten one does not](decisions/D104-a-blocked-instruction-names-its.md) | blocked | 2026-08-20 |
| 🟢 | D105 | [Hand-written assembly is a fixture source, and it caught a wrong row](decisions/D105-hand-written-assembly-is-a-fixture.md) | decided | 2026-08-21 |
| 🟢 | D106 | [The lane model refuses a mask write; Auto reads the shader to choose, and says so](decisions/D106-the-lane-model-refuses-a-mask-write.md) | decided | 2026-08-20 |
| 🟡 | D107 | [The operand solver checks that a family agrees with itself](decisions/D107-the-operand-solver-checks-that-a-family.md) | assumed | ~2026-08-20..08-21 |
| 🟢 | D108 | [An operand the encoding does not carry is recorded, not omitted](decisions/D108-an-operand-the-encoding-does-not-carry.md) | decided | 2026-08-21 |
| 🟡 | D109 | [A candidate operand field may not overlap the opcode](decisions/D109-a-candidate-operand-field-may-not.md) | assumed | ~2026-08-21 |
| 🟢 | D110 | [Control flow is a dispatch loop, not reconstructed structure](decisions/D110-control-flow-is-a-dispatch-loop-not.md) | decided | 2026-08-21 |
| 🟢 | D111 | [Function bodies are exempt from the define-before-use check](decisions/D111-function-bodies-are-exempt-from-the.md) | decided | 2026-08-21 |
| 🟢 | D112 | [The GPU subsystem is driven by submissions, not called by the emulator](decisions/D112-the-gpu-subsystem-is-driven-by.md) | decided | 2026-08-21 |
| 🟢 | D113 | [Translated shaders are cached by content, never by address](decisions/D113-translated-shaders-are-cached-by.md) | decided | 2026-08-20 |
| 🟡 | D114 | [A shader read from memory ends where the program ends](decisions/D114-a-shader-read-from-memory-ends-where.md) | assumed | ~2026-08-20..08-21 |
| 🟢 | D115 | [The condition code is state both models hold](decisions/D115-the-condition-code-is-state-both-models.md) | decided | 2026-08-21 |
| 🟡 | D116 | [`SUPPORTED` lists what the translator handles, not what one function dispatches](decisions/D116-supported-lists-what-the-translator.md) | assumed | ~2026-08-21..08-19 |
| 🟢 | D117 | [Library attribution was fabricated, and looked fine](decisions/D117-library-attribution-was-fabricated-and.md) | decided | 2026-08-19 |
| 🟢 | D118 | [The real library table, found by prediction rather than by guessing](decisions/D118-the-real-library-table-found-by.md) | decided | 2026-08-19 |
| 🟢 | D119 | [A supplied name is not a published one](decisions/D119-a-supplied-name-is-not-a-published-one.md) | decided | 2026-08-19 |
| 🟢 | D120 | [Graphics vocabulary, and the submit function](decisions/D120-graphics-vocabulary-and-the-submit.md) | decided | 2026-08-19 |
| 🟡 | D121 | [Signed overflow is computed in bits, not compared](decisions/D121-signed-overflow-is-computed-in-bits-not.md) | assumed | ~2026-08-19..08-20 |
| 🟢 | D122 | [Knowledge is a file, and it is the loop's output](decisions/D122-knowledge-is-a-file-and-it-is-the-loop.md) | decided | 2026-08-20 |
| 🟢 | D122 | [Which sub-encoding an opcode uses decides how its first word is read](decisions/D122-which-sub-encoding-an-opcode-uses.md) | decided | 2026-08-21 |
| 🟡 | D123 | [An opcode with no shape row is an error, not a skip](decisions/D123-an-opcode-with-no-shape-row-is-an-error.md) | assumed | ~2026-08-21..08-20 |
| 🟢 | D124 | [The biggest wall was C++ static initialisation](decisions/D124-the-biggest-wall-was-c-static.md) | decided | 2026-08-20 |
| 🟡 | D124 | [The division sequence is refused, and says which numbers are missing](decisions/D124-the-division-sequence-is-refused-and.md) | assumed | ~2026-08-20 |
| 🟢 | D125 | [An error code in a pointer register is a wild pointer](decisions/D125-an-error-code-in-a-pointer-register-is.md) | decided | 2026-08-20 |
| 🟡 | D125 | [Two routes to a shader address, and the disagreement is the point](decisions/D125-two-routes-to-a-shader-address-and-the.md) | assumed | ~2026-08-20 |
| 🟡 | D126 | [A submission knows which queue it came from](decisions/D126-a-submission-knows-which-queue-it-came.md) | assumed | ~2026-08-20 |
| 🟢 | D126 | [The word list is harvested and cited; a rule I invented cost the most important name](decisions/D126-the-word-list-is-harvested-and-cited-a.md) | decided | 2026-08-20 |
| 🟡 | D127 | [Source modifiers are read from the raw words, and refused where unimplemented](decisions/D127-source-modifiers-are-read-from-the-raw.md) | assumed | ~2026-08-20 |
| 🟢 | D127 | [Two invented rules in one afternoon, and the tests caught both](decisions/D127-two-invented-rules-in-one-afternoon-and.md) | decided | 2026-08-20 |
| 🟢 | D128 | [A field no probe can reach is widened from its family, and the claim is checked](decisions/D128-a-field-no-probe-can-reach-is-widened.md) | decided | 2026-08-21 |
| 🟢 | D128 | [The heap, and a fault address that identified itself](decisions/D128-the-heap-and-a-fault-address-that.md) | decided | 2026-08-20 |
| 🟢 | D129 | [Side effects on hidden state are part of an instruction, not an extra](decisions/D129-side-effects-on-hidden-state-are-part.md) | decided | 2026-08-21 |
| 🟢 | D129 | [Two progress signals, and the verdict says which moved](decisions/D129-two-progress-signals-and-the-verdict.md) | decided | 2026-08-20 |
| 🟡 | D130 | [The GPU-address assumption is a named function, not a silence](decisions/D130-the-gpu-address-assumption-is-a-named.md) | assumed | ~2026-08-20 |
| 🟡 | D130 | [The thread pointer installs, and it is checked rather than assumed](decisions/D130-the-thread-pointer-installs-and-it-is.md) | assumed | 2026-08-20 |
| 🟡 | D131 | [A capture is a pair: what the call asked for, and the bytes it appended](decisions/D131-a-capture-is-a-pair-what-the-call-asked.md) | assumed | ~2026-08-20 |
| 🟡 | D132 | [Generated fixtures and dumped shaders do not share an extension](decisions/D132-generated-fixtures-and-dumped-shaders.md) | assumed | ~2026-08-20 |
| 🔴 | D133 | [The subgroup level is blocked on hardware, not on effort](decisions/D133-the-subgroup-level-is-blocked-on.md) | blocked | ~2026-08-20 |
| 🟡 | D134 | [A named immediate the reference appends without a comma is still an operand](decisions/D134-a-named-immediate-the-reference-appends.md) | assumed | ~2026-08-20 |
| 🟡 | D135 | [The local data share is workgroup storage, and the lane model refuses it](decisions/D135-the-local-data-share-is-workgroup.md) | assumed | ~2026-08-20 |
| 🟡 | D136 | [The differential oracle is a property, tested by generation](decisions/D136-the-differential-oracle-is-a-property.md) | assumed | ~2026-08-20 |
| 🟡 | D137 | [Out-of-range guest memory is masked, because undefined is not comparable](decisions/D137-out-of-range-guest-memory-is-masked.md) | assumed | ~2026-08-20 |
| 🟡 | D138 | [The generator does not branch](decisions/D138-the-generator-does-not-branch.md) | assumed | ~2026-08-20 |
| 🟢 | D139 | [The target is RDNA2; the supported list names instructions rather than numbering them](decisions/D139-the-target-is-rdna2-the-supported-list.md) | decided | ~2026-08-20 |
| 🟡 | D140 | [A zero-operand entry may be solved from one sample](decisions/D140-a-zero-operand-entry-may-be-solved-from.md) | assumed | ~2026-08-20 |
| 🟡 | D141 | [Sixty-four-lane wavefronts, and the tables do not care](decisions/D141-sixty-four-lane-wavefronts-and-the.md) | assumed | ~2026-08-20 |
| 🟢 | D142 | [One Vulkan loader, one instance, one device, for the life of the process](decisions/D142-one-vulkan-loader-one-instance-one.md) | decided | ~2026-08-20 |
| 🟢 | D143 | [Two thirds of the division sequence, and a sharper reason for the third](decisions/D143-two-thirds-of-the-division-sequence-and.md) | decided | ~2026-08-20 |
| 🟢 | D144 | [The division pre-scale needs no float controls, and could not have had them](decisions/D144-the-division-pre-scale-needs-no-float.md) | decided | ~2026-08-20 |
| 🟢 | D145 | [Wavefront width is a parameter, and a narrow shader is a different instruction stream](decisions/D145-wavefront-width-is-a-parameter-and-a.md) | decided | ~2026-08-20 |
| 🟢 | D146 | [The subgroup level is the per-lane model with a ballot, not a third model](decisions/D146-the-subgroup-level-is-the-per-lane.md) | decided | ~2026-08-20 |
| 🟢 | D147 | [A descriptor this translator cannot address is forced out of bounds](decisions/D147-a-descriptor-this-translator-cannot.md) | decided | ~2026-08-20 |
| 🟢 | D148 | [The shader work gets the same progress loop as the imports](decisions/D148-the-shader-work-gets-the-same-progress.md) | decided | ~2026-08-20 |
| 🟢 | D149 | [An address that resolves is evidence, not just a precondition](decisions/D149-an-address-that-resolves-is-evidence.md) | decided | ~2026-08-20 |
| 🟢 | D150 | [Threads: the guest decides how many, the host decides how fast](decisions/D150-threads-the-guest-decides-how-many-the.md) | decided | 2026-08-20 |
| 🟢 | D151 | [A thread handle is an address, because the guest dereferences it](decisions/D151-a-thread-handle-is-an-address-because.md) | decided | 2026-08-20 |
| 🟢 | D152 | [The entry point was reading a stray host pointer, and it looked like progress](decisions/D152-the-entry-point-was-reading-a-stray.md) | decided | 2026-08-20 |
| 🟢 | D153 | [The process entry image, and a matrix that ruled out three hypotheses at once](decisions/D153-the-process-entry-image-and-a-matrix.md) | decided | 2026-08-20 |
| 🟢 | D154 | [The ranked list is the wrong view at a wall; the ordered tail is the right one](decisions/D154-the-ranked-list-is-the-wrong-view-at-a.md) | decided | 2026-08-20 |
| 🟢 | D155 | [Two names confirmed by hash, and the vocabulary extended to derive them](decisions/D155-two-names-confirmed-by-hash-and-the.md) | confirmed | 2026-08-20 |
| 🟢 | D156 | [Nothing reachable from a guest call may panic](decisions/D156-nothing-reachable-from-a-guest-call-may.md) | decided | 2026-08-20 |
| 🟡 | D157 | [The mapping is parked behind a switch, not deleted and not shipped](decisions/D157-the-mapping-is-parked-behind-a-switch.md) | assumed | 2026-08-20 |
| 🟢 | D158 | [The fault handler always had the registers; it printed two addresses](decisions/D158-the-fault-handler-always-had-the.md) | decided | 2026-08-20 |
| 🟢 | D159 | [Every guest call was misaligned, and nobody was looking](decisions/D159-every-guest-call-was-misaligned-and.md) | decided | 2026-08-20 |
| 🟢 | D160 | [A second shim found the logic the first one had absorbed](decisions/D160-a-second-shim-found-the-logic-the-first.md) | decided | 2026-08-20 |
| 🟢 | D161 | [egui, chosen for the shape of what it has to draw](decisions/D161-egui-chosen-for-the-shape-of-what-it.md) | decided | 2026-08-20 |
| 🟢 | D162 | [A settings pane for a subsystem that does not exist is a dead control](decisions/D162-a-settings-pane-for-a-subsystem-that.md) | decided | 2026-08-20 |
| 🟢 | D163 | [Homebrew pkgs are encrypted, measured; pkg support deferred](decisions/D163-homebrew-pkgs-are-encrypted-measured.md) | measured | 2026-08-20 |
| 🟢 | D164 | [The library list was doing file I/O every frame](decisions/D164-the-library-list-was-doing-file-i-o.md) | decided | 2026-08-20 |
| 🟢 | D165 | [The files were always there; there was nothing to hand them over](decisions/D165-the-files-were-always-there-there-was.md) | decided | 2026-08-20 |
| 🟢 | D166 | [The main bisection lever was wired to nothing](decisions/D166-the-main-bisection-lever-was-wired-to.md) | decided | 2026-08-20 |
| 🟢 | D167 | [The video-output handle, and what a large negative sweep is worth](decisions/D167-the-video-output-handle-and-what-a.md) | decided | 2026-08-20 |
| 🟢 | D168 | [The harvested name list is missing the syscall family](decisions/D168-the-harvested-name-list-is-missing-the.md) | decided | 2026-08-21 |
| 🟢 | D169 | [Three names, a video-out handle, and the wall that has not moved](decisions/D169-three-names-a-video-out-handle-and-the.md) | decided | 2026-08-21 |
| 🟢 | D170 | [Cross-project findings taken, and one declined](decisions/D170-cross-project-findings-taken-and-one.md) | decided | 2026-08-21 |
| 🟢 | D171 | [An out-pointer that is never written has no signature](decisions/D171-an-out-pointer-that-is-never-written.md) | decided | 2026-08-21 |
| 🟢 | D172 | [The fault handler walks the frame chain](decisions/D172-the-fault-handler-walks-the-frame-chain.md) | decided | 2026-08-21 |
| 🟢 | D173 | [Call sites, which principle 9 asked for and nothing recorded](decisions/D173-call-sites-which-principle-9-asked-for.md) | decided | 2026-08-21 |
| 🟢 | D174 | [Physical memory has to alias itself](decisions/D174-physical-memory-has-to-alias-itself.md) | decided | 2026-08-21 |
| 🟢 | D175 | [The filesystem is exonerated, and four walls are not one wall](decisions/D175-the-filesystem-is-exonerated-and-four.md) | decided | 2026-08-21 |
| 🟢 | D176 | [Both container generations parse; the refusal was never tested](decisions/D176-both-container-generations-parse-the.md) | decided | 2026-08-21 |
| 🟢 | D177 | [`abort` must not return, and it was reporting the opposite of the truth](decisions/D177-abort-must-not-return-and-it-was.md) | decided | 2026-08-21 |
| 🟢 | D178 | [The mapping was the witness, not the culprit (resolves D157)](decisions/D178-the-mapping-was-the-witness-not-the.md) | decided | 2026-08-20 |
| 🟢 | D179 | [Findings, because the consumer will not be a person](decisions/D179-findings-because-the-consumer-will-not.md) | decided | 2026-08-21 |
| 🟢 | D180 | [Behavioural provenance, because abstinence is not enforceable](decisions/D180-behavioural-provenance-because.md) | decided | 2026-08-21 |
| 🟢 | D181 | [A run records what it was subject to, so a verdict can be evidence](decisions/D181-a-run-records-what-it-was-subject-to-so.md) | decided | 2026-08-21 |
| 🟢 | D182 | [The compatibility record is the other half of the title file](decisions/D182-the-compatibility-record-is-the-other.md) | decided | 2026-08-21 |
| 🟢 | D183 | [snprintf_s refuses rather than renders what it can](decisions/D183-snprintf-s-refuses-rather-than-renders.md) | decided | 2026-08-21 |
| 🟢 | D184 | [Guards for the instrumentation, because the tests were faithful to the mistake](decisions/D184-guards-for-the-instrumentation-because.md) | decided | 2026-08-21 |
| 🟢 | D185 | [A poisoned stack, so "nobody wrote this" can be measured](decisions/D185-a-poisoned-stack-so-nobody-wrote-this.md) | measured | 2026-08-21 |
| 🟢 | D186 | [printf, because the guest was explaining itself into a void](decisions/D186-printf-because-the-guest-was-explaining.md) | decided | 2026-08-21 |
| 🟢 | D187 | [The stub policy reached only declared imports, and a conclusion rested on it](decisions/D187-the-stub-policy-reached-only-declared.md) | decided | 2026-08-21 |
| 🟢 | D188 | [The shipped symbol database is loaded unless told otherwise](decisions/D188-the-shipped-symbol-database-is-loaded.md) | decided | 2026-08-21 |
| 🟢 | D189 | [The vocabulary was never missing a word; it was missing a shape](decisions/D189-the-vocabulary-was-never-missing-a-word.md) | decided | 2026-08-21 |
| 🟢 | D190 | [One allocation path, because alignment cannot be a special case](decisions/D190-one-allocation-path-because-alignment.md) | decided | 2026-08-21 |
| 🟢 | D191 | [The harvest walker ignored the rule written to fix it](decisions/D191-the-harvest-walker-ignored-the-rule.md) | decided | 2026-08-22 |
| 🟢 | D192 | [`check` says everything before it fails, and an advisory tool cannot end the report](decisions/D192-check-says-everything-before-it-fails.md) | decided | 2026-08-22 |
| 🟢 | D193 | [The titles carry their own names, and the generator could not spell one](decisions/D193-the-titles-carry-their-own-names-and.md) | decided | 2026-08-22 |
| 🟢 | D194 | [The run dumps what the guest was pointing at](decisions/D194-the-run-dumps-what-the-guest-was.md) | decided | 2026-08-22 |
| 🟢 | D195 | [A run widens the grammar it searched with](decisions/D195-a-run-widens-the-grammar-it-searched.md) | decided | 2026-08-22 |
| 🟢 | D196 | [The unknowns are a queue, not a candour exercise](decisions/D196-the-unknowns-are-a-queue-not-a-candour.md) | decided | 2026-08-24 |
| 🟢 | D197 | [The run reports the fault, instead of leaving it in the trace](decisions/D197-the-run-reports-the-fault-instead-of.md) | decided | 2026-08-24 |
| 🟢 | D198 | [Findings for the commonest outcome, and dumps for the ones already implemented](decisions/D198-findings-for-the-commonest-outcome-and.md) | decided | 2026-08-24 |
| 🟢 | D199 | [A guard that cannot see what it is checking is worse than none](decisions/D199-a-guard-that-cannot-see-what-it-is.md) | decided | 2026-08-24 |
| 🟢 | D200 | [The submission entry point takes an address, because a guest has one](decisions/D200-the-submission-entry-point-takes-an.md) | decided | ~2026-08-24..08-21 |
| 🟢 | D201 | [Decision numbers are checked, because more than one session assigns them](decisions/D201-decision-numbers-are-checked-because.md) | decided | 2026-08-21 |
| 🟢 | D202 | [A candidate operand field may not overlap anything the encoding table already reads](decisions/D202-a-candidate-operand-field-may-not.md) | decided | 2026-08-21 |
| 🟢 | D203 | [The typed-buffer format table is measured from the assembler, both directions](decisions/D203-the-typed-buffer-format-table-is.md) | measured | 2026-08-21 |
| 🟢 | D204 | [Typed and untyped buffer accesses share one body, and unmeasured formats are refused](decisions/D204-typed-and-untyped-buffer-accesses-share.md) | measured | 2026-08-21 |
| 🟢 | D205 | [Operands spelled as names have their codes measured, not written down](decisions/D205-operands-spelled-as-names-have-their.md) | measured | 2026-08-21 |
| 🟢 | D206 | [Vendor documentation is consumed freely; the LLVM restriction is about oracles, not licences](decisions/D206-vendor-documentation-is-consumed-freely.md) | decided | 2026-08-21 |
| 🟢 | D207 | [orbistoun implements obSCEne's protocol; it does not shape it](decisions/D207-orbistoun-implements-obscene-s-protocol.md) | decided | 2026-08-21 |
| 🟢 | D208 | [The repository's own layout, audited once the tree stopped being small](decisions/D208-the-repository-s-own-layout-audited.md) | decided | 2026-08-24 |
| 🟢 | D209 | [The table generators are a crate, behind a seam that replays a recording](decisions/D209-the-table-generators-are-a-crate-behind.md) | decided | 2026-08-24 |
| 🟢 | D210 | [A semaphore handle is an int, and the type says so now](decisions/D210-a-semaphore-handle-is-an-int-and-the.md) | decided | 2026-08-24 |
| 🟢 | D211 | [The call recorder is the dispatch ring; the crate declaring one is deleted](decisions/D211-the-call-recorder-is-the-dispatch-ring.md) | decided | 2026-08-24 |
| 🟢 | D212 | [A language-model service, in a crate that knows nothing about orbistoun](decisions/D212-a-language-model-service-in-a-crate.md) | decided | 2026-08-24 |
| 🟢 | D213 | [Harvesting is categorised by what it observed, and the tiers are checked](decisions/D213-harvesting-is-categorised-by-what-it.md) | decided | 2026-08-24 |
| 🟢 | D214 | [Proposals are paired with an oracle; the first one asks for words, not names](decisions/D214-proposals-are-paired-with-an-oracle-the.md) | decided | 2026-08-24 |
| 🟢 | D215 | [The toolbar captures the window, and says that is what it captures](decisions/D215-the-toolbar-captures-the-window-and.md) | decided | 2026-08-24 |
| 🟢 | D216 | [A pattern's size is computed once, not per candidate](decisions/D216-a-pattern-s-size-is-computed-once-not.md) | decided | 2026-08-24 |
| 🟢 | D217 | [The readable window was a page low, and the wall had been unreadable because of it](decisions/D217-the-readable-window-was-a-page-low-and.md) | decided | 2026-08-24 |
| 🟢 | D218 | [Two experiments that eliminated four things and confirmed nothing](decisions/D218-two-experiments-that-eliminated-four.md) | confirmed | 2026-08-24 |
| 🟢 | D219 | [The inference runtime is downloaded, not compiled in; and a proposer samples](decisions/D219-the-inference-runtime-is-downloaded-not.md) | decided | 2026-08-24 |
| 🟢 | D220 | [The diagnostics share plumbing, and one of them is dyed banknotes](decisions/D220-the-diagnostics-share-plumbing-and-one.md) | decided | 2026-08-24 |
| 🟢 | D221 | [Every environment variable is declared in one crate](decisions/D221-every-environment-variable-is-declared.md) | decided | 2026-08-24 |
| 🟢 | D222 | [A build says which build it is, and the field for it was never populated](decisions/D222-a-build-says-which-build-it-is-and-the.md) | decided | 2026-08-24 |
| 🟢 | D223 | [Three cheap diagnostics, and a hypothesis nobody had raised](decisions/D223-three-cheap-diagnostics-and-a.md) | decided | 2026-08-24 |
| 🟢 | D224 | [Mapping the faulting address moved the wall, and the reading was wrong](decisions/D224-mapping-the-faulting-address-moved-the.md) | decided | 2026-08-24 |
| 🟢 | D225 | [Asking the probe is a live oracle, and the answer travels with its caveat](decisions/D225-asking-the-probe-is-a-live-oracle-and.md) | decided | 2026-08-24 |
| 🟢 | D226 | [The correction to D224: the address was wrong after all](decisions/D226-the-correction-to-d224-the-address-was.md) | decided | 2026-08-25 |
| 🟢 | D227 | [Principle 3 applies to the tools, and an intervention says so on the line](decisions/D227-principle-3-applies-to-the-tools-and-an.md) | decided | 2026-08-25 |
| ⚪ | D228 | [The library folder was resolved against the launch directory, which is not a setting](decisions/D228-the-library-folder-was-resolved-against.md) | unrecorded | ~2026-08-25 |
| 🟢 | D229 | [A plant could reach only offset zero, and a fill silently erased it](decisions/D229-a-plant-could-reach-only-offset-zero.md) | decided | 2026-08-25 |
| 🟢 | D230 | [Nothing could change what an unnamed function answers, so an elimination had never been measured](decisions/D230-nothing-could-change-what-an-unnamed.md) | measured | 2026-08-25 |
| 🟢 | D231 | [A chooser is slower than running everything it would choose between](decisions/D231-a-chooser-is-slower-than-running.md) | decided | 2026-08-25 |
| 🟢 | D232 | [A fault that moves and a guest broken earlier are the same address and opposite results](decisions/D232-a-fault-that-moves-and-a-guest-broken.md) | decided | 2026-08-25 |
| 🟢 | D233 | [The subject of a fault is where the guest died, not what it called](decisions/D233-the-subject-of-a-fault-is-where-the.md) | decided | 2026-08-25 |
| 🟢 | D234 | [A forced answer reaches an implemented function, and the implementation still runs](decisions/D234-a-forced-answer-reaches-an-implemented.md) | decided | 2026-08-25 |
| 🟢 | D235 | [The initialiser tags are parsed, and the answer was that there are none](decisions/D235-the-initialiser-tags-are-parsed-and-the.md) | decided | 2026-08-25 |
| 🟢 | D236 | [orbistoun answers the probe's protocol, and is a stand-in for itself](decisions/D236-orbistoun-answers-the-probe-s-protocol.md) | decided | 2026-08-25 |
| 🟢 | D237 | [Address translation was only ever implemented for signed containers](decisions/D237-address-translation-was-only-ever.md) | decided | 2026-08-25 |
| 🟢 | D238 | [The limit that decides a verdict is a call budget; the clock stays as a backstop](decisions/D238-the-limit-that-decides-a-verdict-is-a.md) | decided | 2026-08-25 |
| 🟢 | D239 | [Two counters, one quantity, and the number a person read was wrong by ten](decisions/D239-two-counters-one-quantity-and-the.md) | decided | 2026-08-25 |
| 🟢 | D240 | [The numbers in the documentation are generated, and a check fails when they drift](decisions/D240-the-numbers-in-the-documentation-are.md) | decided | 2026-08-25 |
| 🟢 | D241 | [One lock per module for one process-wide table, and the run that measured nothing](decisions/D241-one-lock-per-module-for-one-process.md) | measured | 2026-08-25 |
| 🟢 | D242 | [A name enters the database only if this repository can re-derive it](decisions/D242-a-name-enters-the-database-only-if-this.md) | decided | 2026-08-25 |
| 🟢 | D243 | [The work list removed what a run solved, not what anything could name](decisions/D243-the-work-list-removed-what-a-run-solved.md) | decided | 2026-08-25 |
| 🟢 | D244 | [An illegal instruction has an address, and it is not one the guest asked for](decisions/D244-an-illegal-instruction-has-an-address.md) | decided | 2026-08-25 |
| 🟢 | D245 | [The probe's by-name census was arriving and going nowhere](decisions/D245-the-probe-s-by-name-census-was-arriving.md) | decided | 2026-08-25 |
| 🟢 | D246 | [An existence fact is graded like every other fact, and only the target may name](decisions/D246-an-existence-fact-is-graded-like-every.md) | decided | 2026-08-25 |
| 🟢 | D247 | [orbistoun read modules by a path the console does not use](decisions/D247-orbistoun-read-modules-by-a-path-the.md) | decided | 2026-08-25 |
| 🟢 | D248 | [An unchanged fault yields two different offsets, so it read as disagreement](decisions/D248-an-unchanged-fault-yields-two-different.md) | decided | 2026-08-25 |
| 🟢 | D249 | [Nothing the guest calls answers with the base, through any channel](decisions/D249-nothing-the-guest-calls-answers-with.md) | decided | 2026-08-25 |
| 🟢 | D250 | [A guest gets writable storage, and it is never the title's own directory](decisions/D250-a-guest-gets-writable-storage-and-it-is.md) | decided | 2026-08-25 |
| 🟢 | D251 | [The console's filesystem is a knowledge file, and a title's data layers over it](decisions/D251-the-console-s-filesystem-is-a-knowledge.md) | decided | 2026-08-25 |
| 🟢 | D252 | [A failed open must not look like a descriptor](decisions/D252-a-failed-open-must-not-look-like-a.md) | decided | 2026-08-25 |
| 🟢 | D253 | [The model was told the wrong libraries, and earned names from ones it was never shown](decisions/D253-the-model-was-told-the-wrong-libraries.md) | decided | 2026-08-25 |
| 🟢 | D254 | [A wrong proposal is free on disk and expensive in the loop](decisions/D254-a-wrong-proposal-is-free-on-disk-and.md) | decided | 2026-08-25 |
| 🟡 | D255 | [Thirty-five per cent of what the model proposed was already in the shipped word list](decisions/D255-thirty-five-per-cent-of-what-the-model.md) | proposed | 2026-08-25 |
| 🟢 | D256 | [The probe's worklist, taken from the top](decisions/D256-the-probe-s-worklist-taken-from-the-top.md) | decided | 2026-08-25 |
| 🟢 | D257 | [A name that does not say how it was found cannot be used](decisions/D257-a-name-that-does-not-say-how-it-was.md) | decided | 2026-08-25 |
| 🟢 | D258 | [The vocabulary was never the gap; the shapes were](decisions/D258-the-vocabulary-was-never-the-gap-the.md) | decided | 2026-08-25 |
| 🟢 | D259 | [Two missing words made seven confirmed names unreproducible](decisions/D259-two-missing-words-made-seven-confirmed.md) | confirmed | 2026-08-25 |
| 🟢 | D260 | [The audit reads the grammar the binary was built with, not the one on disk](decisions/D260-the-audit-reads-the-grammar-the-binary.md) | decided | 2026-08-25 |
| 🟢 | D261 | [Shapes are the binding constraint, not vocabulary, by three to one](decisions/D261-shapes-are-the-binding-constraint-not.md) | decided | 2026-08-25 |
| 🟢 | D262 | [Repeated `learned` became affordable when the list shrank, and nobody noticed](decisions/D262-repeated-learned-became-affordable-when.md) | decided | 2026-08-25 |
| 🟢 | D263 | [A diagnostic clause is read from the right, so a target may be qualified](decisions/D263-a-diagnostic-clause-is-read-from-the.md) | decided | 2026-08-25 |
| 🟢 | D264 | [A shape has two costs, and they rank differently](decisions/D264-a-shape-has-two-costs-and-they-rank.md) | decided | 2026-08-25 |
| 🟢 | D265 | [The model gets its own binary, not a place in the CLI](decisions/D265-the-model-gets-its-own-binary-not-a.md) | decided | 2026-08-25 |
| 🟢 | D266 | [A round's cost depends on which position it grows, by a factor of twenty](decisions/D266-a-round-s-cost-depends-on-which.md) | decided | 2026-08-25 |
| 🟢 | D267 | [A second guess at where data lives put a downloaded runtime in the repository](decisions/D267-a-second-guess-at-where-data-lives-put.md) | decided | 2026-08-25 |
| 🟢 | D268 | [Floating-point arguments never reached an implementation](decisions/D268-floating-point-arguments-never-reached.md) | decided | 2026-08-25 |
| 🟢 | D269 | [A mount replaces and a layer stacks, and the order cost a title its textures](decisions/D269-a-mount-replaces-and-a-layer-stacks-and.md) | decided | 2026-08-25 |
| 🟢 | D270 | [The C library arrived as one batch because it was one absence](decisions/D270-the-c-library-arrived-as-one-batch.md) | decided | 2026-08-25 |
| 🟢 | D271 | [An error code in a boolean reads as true](decisions/D271-an-error-code-in-a-boolean-reads-as-true.md) | decided | 2026-08-25 |
| 🟢 | D272 | [Two shapes a guest hands over, and both were got wrong](decisions/D272-two-shapes-a-guest-hands-over-and-both.md) | decided | 2026-08-25 |
| 🟢 | D273 | [A count, an offset and a descriptor are read as data, not tested against a table](decisions/D273-a-count-an-offset-and-a-descriptor-are.md) | decided | 2026-08-25 |
| 🟢 | D274 | [The first call into guest code, rather than out of it](decisions/D274-the-first-call-into-guest-code-rather.md) | decided | 2026-08-25 |
| 🟢 | D275 | [A clock that does not advance reads as a sleep that returned instantly](decisions/D275-a-clock-that-does-not-advance-reads-as.md) | decided | 2026-08-25 |
| 🟢 | D276 | [The expensive half of a watchpoint, built because the cheap half ran out](decisions/D276-the-expensive-half-of-a-watchpoint.md) | decided | 2026-08-25 |
| 🟢 | D277 | [A data breakpoint fires after the access, and pretending otherwise would be a lie](decisions/D277-a-data-breakpoint-fires-after-the.md) | decided | 2026-08-25 |
| 🟢 | D278 | [A watchpoint that reads its own address traps itself](decisions/D278-a-watchpoint-that-reads-its-own-address.md) | decided | 2026-08-25 |
| 🟢 | D279 | [A dump is binary, and the size guard was measuring the wrong thing](decisions/D279-a-dump-is-binary-and-the-size-guard-was.md) | decided | 2026-08-25 |
| 🟢 | D280 | [The first six steps of `check` could hide the other ten](decisions/D280-the-first-six-steps-of-check-could-hide.md) | decided | 2026-08-25 |
| 🟢 | D281 | [Five crates export a registration nothing calls](decisions/D281-five-crates-export-a-registration.md) | decided | 2026-08-26 |
| 🟢 | D282 | [One body for two arities read an argument it was never passed](decisions/D282-one-body-for-two-arities-read-an.md) | decided | 2026-08-26 |
| 🟢 | D283 | [The wall needed two things right at once, and the sweep varied one at a time](decisions/D283-the-wall-needed-two-things-right-at.md) | decided | 2026-08-26 |
| 🟢 | D284 | [A contract can be measured without knowing what the function means](decisions/D284-a-contract-can-be-measured-without.md) | measured | 2026-08-26 |
| 🟢 | D285 | [The shape that spells a verb over two learned nouns](decisions/D285-the-shape-that-spells-a-verb-over-two.md) | decided | 2026-08-26 |
| 🟢 | D286 | [The sweep gains a second dimension, and it is a condition rather than a sentinel](decisions/D286-the-sweep-gains-a-second-dimension-and.md) | decided | 2026-08-26 |
| 🟢 | D287 | [Naming a function silently breaks every experiment aimed at its hash](decisions/D287-naming-a-function-silently-breaks-every.md) | decided | 2026-08-26 |
| 🟢 | D288 | [The sweep kept its own list of diagnostics, and it was already wrong](decisions/D288-the-sweep-kept-its-own-list-of.md) | decided | 2026-08-26 |
| 🟢 | D289 | [The plan gets a runner, and an out-parameter finding follows itself through](decisions/D289-the-plan-gets-a-runner-and-an-out.md) | decided | 2026-08-26 |
| 🟢 | D290 | [Every floating-point function works and every one is reported as missing](decisions/D290-every-floating-point-function-works-and.md) | decided | 2026-08-26 |
| 🟢 | D291 | [What the sweep measured becomes a knowledge entry, and what it did not becomes an assumption](decisions/D291-what-the-sweep-measured-becomes-a.md) | measured | 2026-08-26 |
| 🟢 | D292 | [The merge rule moves to the crate that owns the format](decisions/D292-the-merge-rule-moves-to-the-crate-that.md) | decided | 2026-08-26 |
| 🟢 | D293 | [The dispatcher and the naming loop become two crates](decisions/D293-the-dispatcher-and-the-naming-loop.md) | decided | 2026-08-26 |
| 🟢 | D294 | [The first function implemented from a contract the loop measured](decisions/D294-the-first-function-implemented-from-a.md) | measured | 2026-08-26 |
| 🟢 | D295 | [A stub policy that can write, so the loop stops needing a person to type Rust](decisions/D295-a-stub-policy-that-can-write-so-the.md) | decided | 2026-08-26 |
| 🟢 | D296 | [Three tiers of automatic fix, and why the third is the smallest](decisions/D296-three-tiers-of-automatic-fix-and-why.md) | decided | 2026-08-26 |
| 🟢 | D297 | [The learned file becomes a record of measurements, so it can be sent to somebody](decisions/D297-the-learned-file-becomes-a-record-of.md) | decided | 2026-08-26 |
| 🟢 | D298 | [Verification runs against a machine that has learned nothing](decisions/D298-verification-runs-against-a-machine.md) | decided | 2026-08-26 |
| 🟢 | D299 | [Finding which answer the guest dereferenced, rather than reasoning about it](decisions/D299-finding-which-answer-the-guest.md) | decided | 2026-08-26 |
| 🟢 | D300 | [One concept: give the guest a region, and say how it arrives](decisions/D300-one-concept-give-the-guest-a-region-and.md) | decided | 2026-08-26 |
| 🟢 | D301 | [`FURTHER` saturates, and a comparison needs headroom to be a comparison](decisions/D301-further-saturates-and-a-comparison.md) | decided | 2026-08-26 |
| 🟢 | D302 | [Make the fix loop look like the naming loop: oracle first, generator second](decisions/D302-make-the-fix-loop-look-like-the-naming.md) | decided | 2026-08-26 |
| 🟢 | D303 | [The corpus is the oracle; the probe is one member of it](decisions/D303-the-corpus-is-the-oracle-the-probe-is.md) | decided | 2026-08-26 |
| 🟢 | D304 | [The repair searched for an index it could compute](decisions/D304-the-repair-searched-for-an-index-it.md) | decided | 2026-08-26 |
| 🟢 | D305 | [A plain name is a NID nobody hashed yet](decisions/D305-a-plain-name-is-a-nid-nobody-hashed-yet.md) | decided | 2026-08-26 |
| 🟢 | D306 | [The payloads take their world from `rdi`, and the stack says nothing](decisions/D306-the-payloads-take-their-world-from-rdi.md) | decided | 2026-08-26 |
| 🟢 | D307 | [An import that names data was getting a function](decisions/D307-an-import-that-names-data-was-getting-a.md) | decided | 2026-08-26 |
| 🟢 | D308 | [Ask the guest which field it wants, rather than guessing the structure](decisions/D308-ask-the-guest-which-field-it-wants.md) | decided | 2026-08-27 |
| 🟢 | D309 | [The comparison invented a position the display half refuses to invent](decisions/D309-the-comparison-invented-a-position-the.md) | decided | 2026-08-27 |
| 🟢 | D310 | [The window and the guest are different processes, so the shell button had nowhere to go](decisions/D310-the-window-and-the-guest-are-different.md) | decided | 2026-08-27 |
| 🟢 | D311 | [The shell holds meanings and refuses to hold the vendor's numbers](decisions/D311-the-shell-holds-meanings-and-refuses-to.md) | decided | 2026-08-27 |
| 🟢 | D312 | [A run helped by named overrides was recorded as an honest measurement](decisions/D312-a-run-helped-by-named-overrides-was.md) | decided | 2026-08-27 |
| 🟢 | D313 | [One word for the shell, and the window had already taken it](decisions/D313-one-word-for-the-shell-and-the-window.md) | decided | 2026-08-27 |
| 🟢 | D314 | [An argument beats a setting, and contradictions are refused rather than resolved](decisions/D314-an-argument-beats-a-setting-and.md) | decided | 2026-08-27 |
| 🟢 | D315 | [A submission is a bundle of claims, and the receiver counts them itself](decisions/D315-a-submission-is-a-bundle-of-claims-and.md) | decided | 2026-08-27 |
| 🟢 | D316 | [The decision number was allocated by a convention that races](decisions/D316-the-decision-number-was-allocated-by-a.md) | decided | 2026-08-27 |
| 🟢 | D317 | ["Both sides are Vulkan" was an assumption, and it happened to be true](decisions/D317-both-sides-are-vulkan-was-an-assumption.md) | decided | 2026-08-27 |
| 🔴 | D318 | [The overlay is blocked earlier than "cross-process presentation"](decisions/D318-the-overlay-is-blocked-earlier-than.md) | blocked | 2026-08-27 |
| 🟡 | D319 | [A gate that cannot be scoped cannot be run in a shared tree](decisions/D319-a-gate-that-cannot-be-scoped-cannot-be.md) | scoped | 2026-08-27 |
| 🟢 | D320 | [A curated list regrew to sixty-seven times its size, and only the clock noticed](decisions/D320-a-curated-list-regrew-to-sixty-seven.md) | decided | 2026-08-26 |
| 🟢 | D321 | [Three things were built and inert, and one of them could not be built at all](decisions/D321-three-things-were-built-and-inert-and.md) | decided | 2026-08-27 |
| 🟢 | D322 | [A generated patch is safe because promotion is the verification step](decisions/D322-a-generated-patch-is-safe-because.md) | decided | 2026-08-27 |
| 🟢 | D323 | [Data imports get storage, not a stub](decisions/D323-data-imports-get-storage-not-a-stub.md) | decided | 2026-08-27 |
| 🟢 | D324 | [Nobody picks a test address, at either level](decisions/D324-nobody-picks-a-test-address-at-either.md) | decided | 2026-08-27 |
| 🟢 | D325 | [The third place nobody wrote, and proving the poison fired](decisions/D325-the-third-place-nobody-wrote-and.md) | decided | 2026-08-27 |
| 🟢 | D326 | [A controller subsystem that stops at the guest, and says where](decisions/D326-a-controller-subsystem-that-stops-at.md) | decided | 2026-08-27 |
| 🟢 | D327 | [Categories along a row, children down a column](decisions/D327-categories-along-a-row-children-down-a.md) | decided | 2026-08-27 |
| 🟢 | D328 | [A promotion generated from a measurement, and the one field it invented](decisions/D328-a-promotion-generated-from-a.md) | decided | 2026-08-27 |
| 🟢 | D329 | [The one table left hand-copied, and the rule that seemed to forbid generating it](decisions/D329-the-one-table-left-hand-copied-and-the.md) | decided | 2026-08-27 |
| 🟢 | D330 | [The harvest could undo the curation, and a one-line list read as seventy-six words](decisions/D330-the-harvest-could-undo-the-curation-and.md) | decided | 2026-08-27 |
| 🟢 | D331 | [The diagnostic that said something was the one the summary dropped](decisions/D331-the-diagnostic-that-said-something-was.md) | decided | 2026-08-27 |
| 🟢 | D332 | [A record with no honest baseline, and the parse test that failed for it](decisions/D332-a-record-with-no-honest-baseline-and.md) | decided | 2026-08-27 |
| 🟢 | D333 | [The model that costs nothing to set up, and the two things not copied with it](decisions/D333-the-model-that-costs-nothing-to-set-up.md) | decided | 2026-08-27 |
| 🟢 | D334 | [Measuring the engines took three tries, and the first two ranked the wrong one](decisions/D334-measuring-the-engines-took-three-tries.md) | decided | 2026-08-27 |
| 🟢 | D335 | [The benchmark was wrong three times, and the third nearly retired a working engine](decisions/D335-the-benchmark-was-wrong-three-times-and.md) | decided | 2026-08-27 |
| 🟢 | D336 | [`/no_think` suppressed the reasoning and not the tags](decisions/D336-no-think-suppressed-the-reasoning-and.md) | decided | 2026-08-27 |
| 🟢 | D340 | [The pad library exports both spellings, and both are imported](decisions/D340-the-pad-library-exports-both-spellings.md) | decided | 2026-08-27 |
| 🟢 | D341 | [Cross-port key conflicts were invisible, and a keyboard could not move a stick](decisions/D341-cross-port-key-conflicts-were-invisible.md) | decided | 2026-08-27 |
| 🟢 | D342 | [A shape can be disabled, and the two names that cost](decisions/D342-a-shape-can-be-disabled-and-the-two.md) | decided | 2026-08-27 |
| 🟢 | D343 | [Enter at `main` and the payloads start working](decisions/D343-enter-at-main-and-the-payloads-start.md) | decided | 2026-08-27 |
| 🟢 | D344 | [Guest threads are asked to stop, not made to](decisions/D344-guest-threads-are-asked-to-stop-not.md) | decided | 2026-08-27 |
| 🟢 | D345 | [Input crosses the process boundary before anything can read it](decisions/D345-input-crosses-the-process-boundary.md) | decided | 2026-08-27 |
| 🟢 | D346 | [A console has users, and one of their names reaches a guest unencoded](decisions/D346-a-console-has-users-and-one-of-their.md) | decided | 2026-08-27 |
| 🟢 | D347 | [A record named after a scratch directory](decisions/D347-a-record-named-after-a-scratch-directory.md) | decided | 2026-08-27 |
| 🟢 | D348 | [klogsrv prints its own banner](decisions/D348-klogsrv-prints-its-own-banner.md) | decided | 2026-08-27 |
| 🟢 | D349 | [The POSIX spellings were unserved, and they are the same functions](decisions/D349-the-posix-spellings-were-unserved-and.md) | decided | 2026-08-27 |
| 🟢 | D350 | [`sysctl` refuses what it does not know, and says what was asked](decisions/D350-sysctl-refuses-what-it-does-not-know.md) | decided | 2026-08-27 |
| 🟢 | D351 | [The sweep could not judge a guest that never faults](decisions/D351-the-sweep-could-not-judge-a-guest-that.md) | decided | 2026-08-27 |
| 🟢 | D352 | [The harvest took names because the work was naming](decisions/D352-the-harvest-took-names-because-the-work.md) | decided | 2026-08-27 |
| 🟢 | D353 | [The harvest is a command, and writing it twice found a bug](decisions/D353-the-harvest-is-a-command-and-writing-it.md) | decided | 2026-08-27 |
| 🟢 | D354 | [A revision the file states is a claim; a revision the checkout states is a fact](decisions/D354-a-revision-the-file-states-is-a-claim-a.md) | decided | 2026-08-27 |
| 🟢 | D355 | [A turn that measured a contract and wrote nothing](decisions/D355-a-turn-that-measured-a-contract-and.md) | measured | 2026-08-27 |
| 🟢 | D356 | [The dispatcher read what crashed and never what we had written down as unknown](decisions/D356-the-dispatcher-read-what-crashed-and.md) | decided | 2026-08-27 |
| 🟢 | D357 | [The loop can close a question when the discriminator is arithmetic](decisions/D357-the-loop-can-close-a-question-when-the.md) | decided | 2026-08-28 |
| 🟢 | D358 | [The loop writes down what it worked out](decisions/D358-the-loop-writes-down-what-it-worked-out.md) | decided | 2026-08-29 |
| 🟢 | D359 | [Entering at `main` skips the initialisation the program needed](decisions/D359-entering-at-main-skips-the.md) | decided | 2026-08-29 |
| 🟢 | D360 | [`.bss` markers that name themselves, and what they found](decisions/D360-bss-markers-that-name-themselves-and.md) | decided | 2026-08-29 |
| 🟢 | D361 | [The documentation route is closed, not untried](decisions/D361-the-documentation-route-is-closed-not.md) | decided | 2026-08-29 |
| 🟢 | D362 | [`concat!` defeats an implicit format capture](decisions/D362-concat-defeats-an-implicit-format.md) | decided | 2026-08-29 |
| 🟢 | D363 | [A compile-time constant fails as though the source were broken](decisions/D363-a-compile-time-constant-fails-as-though.md) | decided | 2026-08-29 |
| 🟢 | D364 | [A va_list is a cursor, so the v-forms render what the register forms cannot](decisions/D364-a-va-list-is-a-cursor-so-the-v-forms.md) | decided | 2026-08-29 |
| 🟢 | D365 | [The handoff structure answers by being asked](decisions/D365-the-handoff-structure-answers-by-being.md) | decided | 2026-08-29 |
| 🟢 | D366 | [sceKernelDlsym is how a payload gets its C library, and the answer is a stub we already had](decisions/D366-scekerneldlsym-is-how-a-payload-gets.md) | decided | 2026-08-29 |
| 🟢 | D367 | [A symbol is declared where it is imported, not where its code lives](decisions/D367-a-symbol-is-declared-where-it-is.md) | decided | 2026-08-29 |
| 🟢 | D368 | [Handoff field two is a pointer, and what the unknown fields hold is a setting](decisions/D368-handoff-field-two-is-a-pointer-and-what.md) | decided | 2026-08-29 |
| 🟢 | D369 | [A marker nobody decodes is arithmetic somebody does by hand](decisions/D369-a-marker-nobody-decodes-is-arithmetic.md) | decided | 2026-08-29 |
| 🟢 | D370 | [A constant one crate hardcodes is checked by the crate that holds the table](decisions/D370-a-constant-one-crate-hardcodes-is.md) | decided | 2026-08-29 |
| 🟢 | D371 | [Files and sockets share one descriptor table, because a guest has one](decisions/D371-files-and-sockets-share-one-descriptor.md) | decided | 2026-08-29 |
| 🟢 | D372 | [Where the shared thing is what is under test, a lock is the fix](decisions/D372-where-the-shared-thing-is-what-is-under.md) | decided | 2026-08-29 |
| 🟢 | D373 | [Asking must not take, and waiting must not hold](decisions/D373-asking-must-not-take-and-waiting-must.md) | decided | 2026-08-29 |
| 🟢 | D374 | [The checkout is a newer FreeBSD than the target](decisions/D374-the-checkout-is-a-newer-freebsd-than.md) | decided | 2026-08-29 |
| 🟢 | D375 | [orbistoun runs a payload built with the real toolchain](decisions/D375-orbistoun-runs-a-payload-built-with-the.md) | decided | 2026-08-29 |
| 🟢 | D376 | [A runtime's globals are named, and the last wall is a syscall gadget](decisions/D376-a-runtime-s-globals-are-named-and-the.md) | decided | 2026-08-29 |
| 🟢 | D377 | [A syscall gadget is not a function](decisions/D377-a-syscall-gadget-is-not-a-function.md) | decided | 2026-08-29 |
| 🟢 | D378 | [The syscall boundary, built and not yet reached](decisions/D378-the-syscall-boundary-built-and-not-yet.md) | decided | 2026-08-29 |
| 🟢 | D379 | [A setting consulted nowhere, for the fourth time](decisions/D379-a-setting-consulted-nowhere-for-the.md) | decided | 2026-08-29 |
| 🟢 | D380 | [A fault in our own code should say where in our own code](decisions/D380-a-fault-in-our-own-code-should-say.md) | decided | 2026-08-29 |
| 🟢 | D381 | [The dispatch path runs on the guest's stack](decisions/D381-the-dispatch-path-runs-on-the-guest-s.md) | decided | 2026-08-29 |
| 🟢 | D382 | [ftpsrv wants to be root, and that is a wall worth having](decisions/D382-ftpsrv-wants-to-be-root-and-that-is-a.md) | decided | 2026-08-29 |
| 🟢 | D383 | [Conforming is not the same as compatible](decisions/D383-conforming-is-not-the-same-as-compatible.md) | decided | 2026-08-29 |
| 🟡 | D384 | [A gadget is not reached from a call site a compiler wrote](decisions/D384-a-gadget-is-not-reached-from-a-call.md) | assumed | 2026-08-30 |
| 🟡 | D385 | [What a harvest skips has to be counted, or the section is a lie](decisions/D385-what-a-harvest-skips-has-to-be-counted.md) | assumed | 2026-08-30 |
| 🟡 | D386 | [The seventh argument is on the stack, and nothing was reading it](decisions/D386-the-seventh-argument-is-on-the-stack.md) | assumed | 2026-08-30 |
| 🟡 | D387 | [A guest thread's stack is guest memory, and the diagnostics could not see it](decisions/D387-a-guest-thread-s-stack-is-guest-memory.md) | assumed | 2026-08-30 |
| 🟡 | D388 | [A set says which and never when, and *when* was the whole question](decisions/D388-a-set-says-which-and-never-when-and.md) | assumed | 2026-08-30 |
| 🟡 | D389 | [`/dev/klog` has something true in it, and the accuracy caveat that comes with it](decisions/D389-dev-klog-has-something-true-in-it-and.md) | assumed | 2026-08-30 |
| 🟡 | D390 | [Ask the guest which fields it uses, one poisoned field per run](decisions/D390-ask-the-guest-which-fields-it-uses-one.md) | assumed | 2026-08-30 |
| 🟢 | D391 | [What real hardware said, and which of it orbistoun had wrong](decisions/D391-what-real-hardware-said-and-which-of-it.md) | hardware | 2026-08-30 |
| 🟡 | D392 | [A stub for every import makes "does this symbol exist" unanswerable](decisions/D392-a-stub-for-every-import-makes-does-this.md) | assumed | 2026-08-30 |
| 🟡 | D393 | [A capital letter is a different symbol](decisions/D393-a-capital-letter-is-a-different-symbol.md) | assumed | 2026-08-30 |
| 🟡 | D394 | [The machine is one setting, not five constants](decisions/D394-the-machine-is-one-setting-not-five.md) | assumed | 2026-08-30 |
| 🟡 | D395 | [Not derivable is not the same as not measurable](decisions/D395-not-derivable-is-not-the-same-as-not.md) | assumed | 2026-08-30 |
| 🟡 | D396 | [A log written after the guest stops is a log no guest can read](decisions/D396-a-log-written-after-the-guest-stops-is.md) | assumed | 2026-08-30 |
| 🟡 | D397 | [The kernel's own version is a setting with no default](decisions/D397-the-kernel-s-own-version-is-a-setting.md) | assumed | 2026-08-30 |
| 🟢 | D398 | [The hardware trip happened, and it moved seven placeholders](decisions/D398-the-hardware-trip-happened-and-it-moved.md) | hardware | 2026-08-30 |
| 🟢 | D399 | [The payloads were never handed what they read, and the instrument could not see it](decisions/D399-the-payloads-were-never-handed-what.md) | measured | 2026-08-30 |
| 🟢 | D400 | [The payload builds its own syscall entry, and this hands it something with no inside](decisions/D400-the-payload-builds-its-own-syscall.md) | measured | 2026-08-30 |
| 🟢 | D401 | [A guest that talks to the kernel directly left no trace on the work list](decisions/D401-a-guest-that-talks-to-the-kernel.md) | measured | 2026-08-30 |
| 🟢 | D402 | [A stub that returns from `exit` turns a clean shutdown into a crash](decisions/D402-a-stub-that-returns-from-exit-turns-a.md) | measured | 2026-08-30 |
| 🟢 | D403 | [The call that was blocking every payload, and what it says the machine is](decisions/D403-the-call-that-was-blocking-every.md) | measured | 2026-08-30 |
| ⚪ | D404 | [The wall behind 649: firmware-specific address arithmetic](decisions/D404-the-wall-behind-649-firmware-specific.md) | unrecorded | 2026-08-30 |
| 🟢 | D405 | [The console answered the sysctl probe, and it says 12.40](decisions/D405-the-console-answered-the-sysctl-probe.md) | measured | 2026-08-30 |
| 🟡 | D406 | [A firmware skeleton crate, for guests that reach past the interface](decisions/D406-a-firmware-skeleton-crate-for-guests.md) | assumed | 2026-08-30 |
| 🟢 | D407 | [Word zero is getpid, and libkernel is laid out by measured vaddr](decisions/D407-word-zero-is-getpid-and-libkernel-is.md) | measured | 2026-08-30 |
| 🟢 | D408 | [The handoff, measured whole on a console, and made faithful here](decisions/D408-the-handoff-measured-whole-on-a-console.md) | measured | 2026-08-31 |
| 🟡 | D409 | [Layout as a testable plan, console profiles, port reporting, and vaddr provenance](decisions/D409-layout-as-a-testable-plan-console.md) | assumed | 2026-08-31 |
| 🟢 | D410 | [The console confirmed five more export vaddrs, and validated the whole base+vaddr model](decisions/D410-the-console-confirmed-five-more-export.md) | confirmed | ~2026-08-31 |
| 🟢 | D411 | [Diagnosis of the `image+0x2708` wall in `klog.elf`: kernel_copyout, setsockopt, and high-half kpipe_addr](decisions/D411-diagnosis-of-the-image-0x2708-wall-in.md) | measured | 2026-08-31 |
| ⚪ | D412 | [Syscall 477 (`mmap`) implemented and wired to the syscall table](decisions/D412-syscall-477-mmap-implemented-and-wired.md) | unrecorded | 2026-08-31 |
| 🟢 | D413 | [Emulation of the kernel escape R/W pipe and dynamic symbol resolution](decisions/D413-emulation-of-the-kernel-escape-r-w-pipe.md) | measured | 2026-08-31 |
| 🟡 | D414 | [The test corpus, made a verb: a manifest of sources, fetched and recorded on demand](decisions/D414-the-test-corpus-made-a-verb-a-manifest.md) | assumed | 2026-08-31 |
| 🟡 | D415 | [The compatibility table as a tracked markdown file, generated from the records](decisions/D415-the-compatibility-table-as-a-tracked.md) | assumed | 2026-08-31 |
| 🟢 | D416 | [Four HLE fixes from the hardware-vs-orbistoun obSCEne diff](decisions/D416-four-hle-fixes-from-the-hardware-vs.md) | hardware | 2026-08-31 |
| 🟢 | D417 | [Three more HLE fixes from the same diff: thread join, mutex type, audio init](decisions/D417-three-more-hle-fixes-from-the-same-diff.md) | measured | 2026-08-31 |
| 🟢 | D418 | [The census control failed because obSCEne's canary leaked into our symbol database](decisions/D418-the-census-control-failed-because.md) | measured | 2026-08-31 |
| 🟢 | D419 | [Five more libkernel vaddrs behaviourally confirmed from the second payload run](decisions/D419-five-more-libkernel-vaddrs.md) | confirmed | 2026-08-31 |
| 🟢 | D420 | [sceKernelGetSystemSwVersion answers 13.09, which is not the 12.40 firmware](decisions/D420-scekernelgetsystemswversion-answers-13.md) | measured | 2026-08-31 |
| 🟡 | D421 | [The software version is a profile setting, like the firmware, not a kernel constant](decisions/D421-the-software-version-is-a-profile.md) | assumed | 2026-08-31 |
| 🟢 | D422 | [The console's writable device paths are a per-title sandbox, in the overlay already built](decisions/D422-the-console-s-writable-device-paths-are.md) | decided | 2026-08-31 |
| 🟢 | D423 | [The sandbox is one entry point in orbistoun-fs, not orchestration in a consumer](decisions/D423-the-sandbox-is-one-entry-point-in.md) | decided | 2026-08-31 |
| 🔴 | D424 | [The flip queue is a counter that completes on submit, and it unblocked the whole suite](decisions/D424-the-flip-queue-is-a-counter-that.md) | blocked | 2026-08-31 |
| 🟡 | D425 | [sceVideoOutGetResolutionStatus presents 1080p; the skip is a held output, not a headless one](decisions/D425-scevideooutgetresolutionstatus-presents.md) | assumed | 2026-08-31 |
| 🟢 | D426 | [Video-out refuses with its own error family, and refuses a second open of a held output](decisions/D426-video-out-refuses-with-its-own-error.md) | measured | 2026-09-01 |
| 🟢 | D427 | [The GPU subsystem starts at the command builders: a PM4 writer, and the two dispatch calls](decisions/D427-the-gpu-subsystem-starts-at-the-command.md) | measured | 2026-09-01 |
| 🟢 | D428 | [libSceSysmodule, and the real-title walls triaged](decisions/D428-libscesysmodule-and-the-real-title.md) | measured | 2026-09-01 |
| 🟢 | D429 | [sceKernelReserveVirtualRange, and the boundary the real titles reach](decisions/D429-scekernelreservevirtualrange-and-the.md) | measured | 2026-09-01 |
| 🟢 | D430 | [Reentrant guest execution, and its first user: std::call_once](decisions/D430-reentrant-guest-execution-and-its-first.md) | measured | 2026-09-01 |
| 🟢 | D431 | [The C-runtime threading family, and a metric that fell as it was fixed](decisions/D431-the-c-runtime-threading-family-and-a.md) | measured | 2026-09-01 |
| 🟢 | D432 | [The fault report carries the faulting instruction, and what the shared wall actually is](decisions/D432-the-fault-report-carries-the-faulting.md) | measured | 2026-09-01 |
| 🟢 | D433 | [Guest thread-local storage: the block is built, and Windows will not keep the base](decisions/D433-guest-thread-local-storage-the-block-is.md) | measured | 2026-09-01 |
| 🟢 | D434 | [The Windows thread-pointer backstop, and PPSA28061 past its TLS wall](decisions/D434-the-windows-thread-pointer-backstop-and.md) | measured | 2026-09-01 |
| 🟢 | D435 | [libSceUlt mutexes and condition variables, and the wall that turned out to be online](decisions/D435-libsceult-mutexes-and-condition.md) | measured | 2026-09-01 |
| 🟢 | D436 | [The remaining title walls are one class: a guest allocation returning null, and it needs hardware](decisions/D436-the-remaining-title-walls-are-one-class.md) | hardware | 2026-09-01 |
| 🟢 | D437 | [The hardware memory data was already captured; the map does not start at zero](decisions/D437-the-hardware-memory-data-was-already.md) | hardware | 2026-09-01 |
| 🟢 | D438 | [Cross-section diff against hardware: placeholder error codes replaced with the measured ones](decisions/D438-cross-section-diff-against-hardware.md) | hardware | 2026-09-01 |
| 🟢 | D439 | [Cross-section hardware diff: the system-wide divergences worth fixing](decisions/D439-cross-section-hardware-diff-the-system.md) | hardware | 2026-09-01 |
| 🟢 | D440 | [The hardware cross-section diff is mined out for clean fixes](decisions/D440-the-hardware-cross-section-diff-is.md) | hardware | 2026-09-01 |
| 🟢 | D441 | [dlsym handle validation and sysmodule id-0, from user direction on the D440 tail](decisions/D441-dlsym-handle-validation-and-sysmodule.md) | measured | 2026-09-01 |
| 🟢 | D442 | [The flexible-memory "shared allocator" theory was aimed at the wrong titles](decisions/D442-the-flexible-memory-shared-allocator.md) | measured | 2026-09-01 |
| 🟢 | D443 | [PPSA02664's allocator wall was orbistoun's policy region sitting on the guest's heap](decisions/D443-ppsa02664-s-allocator-wall-was.md) | measured | 2026-09-01 |
| 🟢 | D444 | [obSCEne is orbistoun's oracle now, and it measured the flexible-memory bug exactly](decisions/D444-obscene-is-orbistoun-s-oracle-now-and.md) | measured | 2026-09-01 |
| 🟢 | D445 | [A readable guard above the stack, and obSCEne now runs its whole suite under orbistoun](decisions/D445-a-readable-guard-above-the-stack-and.md) | measured | 2026-09-01 |
| 🟢 | D446 | [sceKernelVirtualQuery now sees the guest's own image and stack](decisions/D446-scekernelvirtualquery-now-sees-the.md) | measured | 2026-09-01 |
| 🟢 | D447 | [sysctlbyname answers the knobs it can source, and kern.osrelease stops being refused](decisions/D447-sysctlbyname-answers-the-knobs-it-can.md) | measured | 2026-09-01 |
| 🟢 | D448 | [The obSCEne oracle's clean orbistoun bugs are mined out; the rest are by design](decisions/D448-the-obscene-oracle-s-clean-orbistoun.md) | measured | 2026-09-01 |
| 🟢 | D449 | [PPSA02664 regressed to the allocator wall; the policy-region/reserve collision is the mechanism (open)](decisions/D449-ppsa02664-regressed-to-the-allocator.md) | measured | 2026-09-01 |
| 🟢 | D450 | [PPSA02664's "regression" is a thread race between two concurrent walls, not a regression](decisions/D450-ppsa02664-s-two-walls-are-a-thread.md) | measured | 2026-09-01 |
| 🟢 | D451 | [PPSA21564 boots once the sceLibcMspace allocator family (and bcmp) answer real values](decisions/D451-ppsa21564-boots-the-mspace.md) | measured | 2026-09-01 |
| 🟢 | D452 | [scePthreadGetthreadid answers the thread's handle, and PPSA21564 reaches 0% stubs](decisions/D452-scepthreadgetthreadid-and-ppsa21564.md) | measured | 2026-09-01 |
| 🟢 | D453 | [POSIX thread-specific-data keys; PPSA21564's TBB scheduler stops aborting](decisions/D453-posix-tls-keys-tbb-unblocked.md) | measured | 2026-09-01 |
| 🟢 | D454 | [localtime/asctime and fgets carry PPSA21564 into main(); it prints, parses its args, and walls on a threading assert](decisions/D454-time-and-fgets-astro-reaches-main.md) | measured | 2026-09-01 |
| 🟢 | D455 | [POSIX unnamed semaphores (sem_init family); the Cond.cpp wall is not a traced HLE call](decisions/D455-posix-unnamed-semaphores.md) | measured | 2026-09-01 |
| 🟢 | D456 | [Fault reports name privileged instructions and call an emulator bug an emulator bug](decisions/D456-fault-reports-name-privileged-instructions.md) | measured | 2026-09-01 |
| 🟢 | D457 | [Fault reports name the null base register, not just dump sixteen](decisions/D457-fault-reports-name-the-null-base-register.md) | measured | 2026-09-01 |
| 🟢 | D458 | [Execute breakpoints, so a guest-computed value can be read where it is used](decisions/D458-execute-breakpoints-capture-call-arguments.md) | measured | 2026-09-01 |
| 🟢 | D459 | [The call trace records what each call answered, not only what it was asked](decisions/D459-trace-records-what-a-call-answered.md) | measured | 2026-09-01 |
| 🟢 | D460 | [Mapping direct memory commits into an existing reservation, it does not reserve again](decisions/D460-map-commits-into-a-reserved-range.md) | measured | 2026-09-01 |

| | meaning |
|---|---|
| 🟢 | settled, and the reasoning rests on something checkable |
| 🟡 | assumed or proposed - made without input, and in the review queue |
| 🔴 | reversed, superseded or blocked |
| ⚪ | no status recorded |

A date with `~` is **not recorded** - it is worked out from the dated entries either
side, because an entry between two of them was written between their dates. `~` alone
is a day both neighbours agree on; `~a..b` is a span, and no day inside it is claimed;
`~>a` and `~<a` are entries with a dated neighbour on only one side. A bare `-` has no
dated entry either side to reason from.
