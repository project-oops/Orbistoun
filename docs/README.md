# orbistoun documentation

orbistoun is a high-level hardware emulator in Rust. Guest x86-64 code runs
natively - the hardware CPU and your CPU are the same architecture - so the work is
reimplementing the operating system underneath it and translating GPU command
streams to Vulkan.

New here? The [root README](../README.md) has the pitch and a one-command start, and
**[THE_LOOP.md](THE_LOOP.md) explains what the tool actually does**, start to finish,
including which steps still need a person. This page is the index to everything deeper.


## The words

- [GLOSSARY.md](GLOSSARY.md) - HLE, thunks, stubs, workers, and the two words that mean
  something else in obSCEne (`shape` here is an instruction's operand layout, not an artifact
  form). Start here if the vocabulary is new; the collection's glossary covers standard ELF
  and `DT_`/`PT_`.

## Repository layout

**[CRATES.md](CRATES.md)** says what each crate is *for* - the dependency spine first, then
execution, graphics, and tooling. It was the root README's "Workspace layout" section.

```
crates/         Cargo workspace members (see CRATES.md)
docs/           This documentation
docs/decisions/ One file per decision; DECISIONS.md is the generated index
docs/worklog/   One file per entry, in document order
docs/backlog/   One file per item, with a status column in the index
docs/roadmap/   One file per phase, likewise
tools/          Offline generators, mostly for the shader work - see tools/README.md
compat/         Per-title compatibility records - what we learned, always tracked
symbols/        Symbol databases produced by the name search - ours, so tracked
titles/         Guest modules - never tracked, and nothing here ever will be
frontend/web/   Static landing page; no build step
assets/         Logo and shared images
.githooks/      Pre-push static-analysis gate
```

**Why `crates/`, not `src/`:** in Cargo a workspace groups its member crates under
`crates/` - this is the Rust convention. `src/` in Rust is a *single* crate's
source. The repo root holds only the manifests and tool config that must live there
(`Cargo.toml`, `rust-toolchain.toml`, `deny.toml`, licences); no application code
sits at the root.

## Architecture

Two ideas do most of the work.

**The dependency spine.** `core` -> `elf` -> `nid` -> `mem` -> `hle` ->
`loader`, and only then the subsystem shims. This order is not stylistic: a
subsystem shim is never *reached* until a guest has loaded, allocated, and spawned
threads, so writing the audio shim before the address space works produces code
that cannot be exercised.

**Interception is linking.** Guest modules import by NID - a 64-bit hash of the
symbol name - and the loader resolves each one against the HLE registry, writing
the result into the guest's relocation slots. There is no hooking pass. A
consequence worth stating plainly: the complete list of what a title needs is
available *statically*, before any guest instruction runs.

## The honest status

No emulator for this target anywhere runs a commercial game. orbistoun is younger than
the others, and what it does today is that every executable in the local corpus loads,
links, and **executes real guest code** - while rendering nothing and having never
spawned a guest thread.

Detail, deliberately unflattering, is in [PROJECT_STATUS.md](PROJECT_STATUS.md). The
number that matters is the share of calls answered by a real implementation rather than
a placeholder - not a screenshot, and not a raw call count, which *rises* when stubs
start lying.

## Reference

**Guide**

Written for somebody using orbistoun rather than changing it. These four were in the tree
and in nothing's index, so the published docs listed them under Guide and this page did not
mention they existed.

- [features/running.md](features/running.md) - running a title, and reading the report. The
  point of a run is not that it worked, but what the guest asked for and what it got.
- [features/library.md](features/library.md) - what orbistoun has found that it can try to
  run, and how it decides.
- [features/naming.md](features/naming.md) - names and hashes: why some library entries show
  a readable function and others a bare number.
- [features/paths.md](features/paths.md) - where orbistoun writes, on each platform, and how
  to move it.

**Start here**
- [THE_LOOP.md](THE_LOOP.md) - what one turn of the work does, start to finish, and
  which steps are still a person's job.
- [WORKFLOW.md](WORKFLOW.md) - the commands that turn that loop: what to run, in what
  order, how often.
- [CLAUDE.md](../CLAUDE.md) - build principles and conventions.

**Build and contribute**
- [BUILDING.md](BUILDING.md) - `bin/orbistoun`: what you need installed, every verb, what
  `check` actually runs and why two of its steps are re-run rather than trusted, and what CI
  runs. Start here before a first build.
- [ACKNOWLEDGEMENTS.md](../ACKNOWLEDGEMENTS.md) - reference-only credit list.
- [DECISIONS.md](DECISIONS.md) - a generated index over `decisions/`, one file per entry,
  with a status column. Every decision and its reasoning, plus the review queue of
  assumptions made without input.
- [WORKLOG.md](WORKLOG.md) - a generated index over `worklog/`, in document order. What was done, with the
  surprises worth knowing. Read with DECISIONS.md at the start of any session.
- [TESTING.md](TESTING.md) - the test strategy, and the oracle problem it works around.
- [SYMBOLS.md](SYMBOLS.md) - symbol database format and the NID hash suffix.
- [PROVENANCE.md](PROVENANCE.md) - how a symbol name is shown to be ours.
- API reference - `cargo doc --workspace --open`, or the published `/doc/` on the site.

**Project**
- [PROJECT_STATUS.md](PROJECT_STATUS.md) - what works today.
- [ROADMAP.md](ROADMAP.md) - a generated index over `roadmap/`, with a status column. Committed next steps, in order.
- [BACKLOG.md](BACKLOG.md) - a generated index over `backlog/`, with a status column. Everything else worth not forgetting.
- [PAYLOADS.md](PAYLOADS.md) - running the open-toolchain payloads, and what it would take
  for Prosperous to drive this the way it drives the hardware.
- [SCOPE.md](SCOPE.md) - what orbistoun deliberately is *not*.
- [REFERENCES.md](REFERENCES.md) - every external document this project relies on,
  what was taken from each, and how it was checked.
- [HANDOVER-OBSCENE.md](HANDOVER-OBSCENE.md) - what orbistoun wants measured next, going the
  other way from obSCEne's handover, so neither side rediscovers what the other knows.

## Next steps

- **Just looking?** `cargo run -p orbistoun-cli -- symbols`, then
  `cargo run -p orbistoun-cli -- questions` for the other half of the picture: what is
  declared, and what is still guessed at.
- **Want to add a system library?** One `guest_module!` block and one line in
  `modules()` in
  [orbistoun-service/src/symbols.rs](../crates/orbistoun-service/src/symbols.rs). See
  [orbistoun-hle](../crates/orbistoun-hle/README.md).
- **Want to move the needle?** The three current walls are in
  [PROJECT_STATUS.md](PROJECT_STATUS.md), and `orbistoun-cli worklist` ranks everything
  behind them by how often a guest actually calls it. Naming work needs no walls at all:
  `crates/orbistoun-names/data/vendor.toml` is the vocabulary, and every word added to it
  reaches every title.

## Adding to a log

DECISIONS, WORKLOG, BACKLOG and ROADMAP are **directories with a generated index**. Add a file
under `decisions/`, `worklog/`, `backlog/` or `roadmap/`, then regenerate its table:

```bash
tools/split-decisions.sh --index orbistoun
tools/split-doc.sh --index orbistoun BACKLOG 3 backlog
```

Do not edit an index by hand - it is overwritten, and the splitter refuses to run over one.
`check-decisions.sh` also fails if a `## Dnnn` heading appears in the index, because an entry
written there is read by nothing and lost on the next regeneration.

The split exists because two sessions appending to one file collide, and because this log
reached 1,069,818 bytes - past the point where GitHub renders markdown at all.
