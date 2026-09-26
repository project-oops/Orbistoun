# Orbistoun documentation

Orbistoun is a high-level emulator in Rust. Guest x86-64 code runs natively, so the work is
reimplementing the operating system and libraries beneath it and translating GPU command
streams to Vulkan. The [root README](../README.md) covers building and a first run; this page
indexes the reference.

## Using Orbistoun

- [features/README.md](features/README.md) - each GUI screen beside its command-line equivalent.
- [features/user-guide.md](features/user-guide.md) - system requirements, running a title, local storage.
- [features/library.md](features/library.md) - the title library and title discovery.
- [features/running.md](features/running.md) - running a title and reading the result.
- [features/inspector.md](features/inspector.md) - the live call trace and HLE resolution inspector.
- [features/memory.md](features/memory.md) - the guest virtual memory layout and register viewer.
- [features/graphics.md](features/graphics.md) - Vulkan settings, shader lowering and detiling.
- [features/controllers.md](features/controllers.md) - controller and keyboard mapping.
- [features/naming.md](features/naming.md) - why imports show as hashes, and how names are recovered.
- [features/paths.md](features/paths.md) - where Orbistoun reads and writes, and portable mode.

## How the work runs

- [THE_LOOP.md](THE_LOOP.md) - one turn of the loop, start to finish, and which steps are a person's.
- [WORKFLOW.md](WORKFLOW.md) - the commands that turn the loop, and in what order.
- [GLOSSARY.md](GLOSSARY.md) - HLE, thunks, stubs, workers, and words that mean something else in obSCEne.
- [SCOPE.md](SCOPE.md) - what Orbistoun deliberately is not.

## Building and contributing

- [BUILDING.md](BUILDING.md) - `bin/orbistoun`: prerequisites, every verb, what `check` and CI run.
- [CRATES.md](CRATES.md) - what each workspace crate is for, in dependency order.
- [TESTING.md](TESTING.md) - the test strategy and the oracles it relies on.
- [ADDRESS_MAP.md](ADDRESS_MAP.md) - every fixed guest base and its owner, gated against the source (D513).
- [SYMBOLS.md](SYMBOLS.md) - the symbol database format and the NID hash suffix.
- [PROVENANCE.md](PROVENANCE.md) - how a symbol name is shown to be derived here.
- [REFERENCES.md](REFERENCES.md) - the external documents relied on, what each supplied, how it was checked.
- [PAYLOADS.md](PAYLOADS.md) - running payloads built with the open toolchain.
- [../ACKNOWLEDGEMENTS.md](../ACKNOWLEDGEMENTS.md) - sources consulted.
- [../CLAUDE.md](../CLAUDE.md) - agent rules for this repository.
- API reference - `cargo doc --workspace --open`.

## Records

- [DECISIONS.md](DECISIONS.md) - the generated index of the decisions in `decisions/`.
- [WORKLOG.md](WORKLOG.md) - one entry per milestone.
- [PROJECT_STATUS.md](PROJECT_STATUS.md) - the generated status page.
- [../COMPATIBILITY.md](../COMPATIBILITY.md) and `titles/` - the generated per-title table and
  pages, rendered from `compat/` by `orbistoun-cli compat markdown`.

A new decision starts with `./bin/orbistoun decide "Noun-phrase title"`, which reserves the next
number and creates `decisions/Dnnn-<slug>.md`. After editing it, regenerate the index from the
collection root with `tools/split-decisions.sh --index orbistoun`. Never edit `DECISIONS.md` by
hand.

## Repository layout

```
crates/          Cargo workspace members (see CRATES.md)
docs/            this documentation
docs/decisions/  one file per decision; DECISIONS.md is the generated index
docs/features/   the user guide, one page per screen
docs/titles/     generated per-title pages
tools/           offline generators and validators (see tools/README.md)
compat/          per-title compatibility records
corpus/          the test-guest manifest and input scripts
symbols/         generated symbol databases
frontend/web/    static landing page, no build step
assets/          logo and shared images
.githooks/       pre-push static-analysis gate
```

Guest material is never in the repository; it lives in the title library outside it.
