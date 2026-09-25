# CLAUDE.md

Read [AGENTS.md](../AGENTS.md), [CONVENTIONS](../docs/CONVENTIONS.md) and
[STYLE](../docs/STYLE.md) first. This file only adds what is specific to orbistoun.

## Mission

A high-level emulator in Rust. Guest x86-64 code runs natively; the work is reimplementing
the target operating system beneath it and translating GPU command streams to Vulkan. No
firmware, no keys, no derived code.

## Start here

```bash
./bin/orbistoun doctor --fix  # is this machine ready; --fix installs what is missing
./bin/orbistoun check         # is the tree sound; the gate before anything is done
./bin/orbistoun run <title>   # one turn of the work
```

The loop:

1. `./bin/orbistoun run <title>` - see where the guest stops and what it asked for.
2. Read the ranked findings; each ends in an arrow saying what to do.
3. Implement it (a subsystem crate) or name it (extend
   `crates/orbistoun-names/data/vendor.toml`) if it is still a bare hash.
4. Run again. The `progress` block says `FURTHER`, `same` or `BACK`.
5. `orbistoun-cli learn` anything the turn established.
6. `./bin/orbistoun check`.

**`FURTHER`** means the guest executed code it could not reach before. It is the measure of
progress. [docs/THE_LOOP.md](docs/THE_LOOP.md) describes each step;
[docs/WORKFLOW.md](docs/WORKFLOW.md) is the command reference.

- Check obSCEne's `docs/` ([repository](https://github.com/project-oops/obSCEne)) before
  starting an investigation; it often already answers the question.
- The decision log is a reference, not an introduction. Consult it when a choice seems
  arbitrary.

## Build principles

### Provenance enforcement

- The `provenance` CI job fails the build on any excluded material.
- Every recorded behaviour carries **`known_by`**: `published`, `measured`,
  `guest-observed` or `assumed`. `orbistoun-cli learn` refuses an entry without one, and CI
  refuses a tree without one.
- `assumed` is a normal state; a written-down assumption can be counted, ranked, probed and
  retired.

### Naming

- The symbol and library name strings inside `guest_module!` declarations are ABI
  identifiers (the NID is computed from them) and stay as they are.
- Our own types carry no vendor prefix: `GuestError`, `GuestResult`, `guest_module!`,
  `is_vendor_segment`.

### Honest failure

- `StubPolicy` defaults to `Unimplemented`, never `Ok`.
- `GuestError` placeholder codes sit in a reserved range no real firmware value occupies
  (`0xF7FF_0000`, D670).
- `Container::imports` errors rather than returning an empty list.
- `orbistoun-env` records which diagnostics intervene in the program, and the run report
  says so.
- **A wall is orbistoun's until hardware proves it the title's** (D708). The default
  attribution of any guest fault is missing or wrong HLE. Blaming the title needs a hardware
  observation that the same path fails the same way there. A guest `TODO:` print, an
  unresolved import, and an obSCEne "not exported on FW *n*" are not that evidence. The
  compat record's `[hardware]` attestation is the ground truth, and the run report frames
  every fault by it.

### Unsafe

- `undocumented_unsafe_blocks`, `multiple_unsafe_ops_per_block` and
  `unsafe_op_in_unsafe_fn` are deny.
- A `// SAFETY:` comment states the invariant that makes the block sound, not what it does.
- Guest memory access is confined to `orbistoun-mem`; everything above it uses safe, checked
  accessors. A subsystem crate that needs a raw pointer has the abstraction in the wrong
  place.
- Parsing never uses `unsafe`. `orbistoun-elf` handles hostile bytes and uses `zerocopy` to
  validate size and alignment.

### Architecture

- **Policy in data.** Stub returns, symbol databases and the NID hash suffix are runtime
  inputs; nothing about a specific title or firmware version is compiled in. If answering
  "what does this function return?" needs a rebuild, it is in the wrong place.
- **Dependency spine.** `core` -> `elf` -> `nid` -> `mem` -> `hle` -> `loader`, then
  subsystems. A subsystem shim is not written before a guest can reach it.
- **Interception is linking.** The loader resolves each NID import and writes the address
  into the guest relocation slot. There is no instrumentation pass, hook, patch or
  trampoline step.
- **Tests.** Pure contracts come first, test-first: NID hashing, address-space validation,
  policy resolution, container parsing. Prefer a pure decision function plus a thin
  effectful wrapper, as in `orbistoun-mem`.
- **Traces.** Binary and indexed; every event carries a global monotonic sequence number and
  the guest return address. Recording does not allocate and never blocks a guest thread.
- **Best end state over speed.** There is no deadline; a shortcut that constrains the design
  is never worth the time it saves (D028).

### Contracts at guest semantics

- A second graphics, audio, input or filesystem backend never requires surgery.
- A string crossing a boundary is a named constant declared once.
- Abstract at what the guest asks for, not what the host API provides; each backend maps
  guest semantics onto its own primitives.
- A seam is structural if it buys testability or swappability now, and speculation if it
  pays off only hypothetically.
- Enforce with crate boundaries: `orbistoun-gpu` does not depend on `ash`.
- There is no execution-backend abstraction and no container-format plugin layer.

### Shims hold no logic

`orbistoun-cli`, `orbistoun-gui` and worker mode are interaction shims over the crates. None
is privileged or holds behaviour the others lack. Shared orchestration lives in
`orbistoun-service`; the shim-to-worker protocol is serialisable data in `orbistoun-proto`,
defined separately from its transport.

## Ground truth

Prefer these sources, in order. A change that none of them justifies says so in its commit
message.

1. FreeBSD source - much of the target C library is POSIX under vendor naming.
2. Framebuffer diffing - render and compare numerically, for the GPU layer.
3. The guest itself - one bit per call site (return `Ok`, does it proceed?), at the cost of a
   boot per query.
4. Instruction test suites - total ground truth, for retro targets; used to validate
   tooling.

## Working practice

- Assume freely on implementation: new crates, splitting, layout, naming and structure need
  no approval. Stop for a new concept not already in the decision log - a mechanism, a
  user-visible behaviour or a subsystem. Adding a crate is not a new concept; adding a
  plugin system is.
- `./bin/orbistoun decide "title"` reserves a decision number atomically and creates the
  file. It does not index it: regenerate the index from the collection root with
  `tools/split-decisions.sh --index orbistoun`. `./bin/orbistoun decisions` warns about
  written but unlisted entries.
- `docs/DECISIONS.md` and `docs/WORKLOG.md` are generated (the worklog from
  `docs/worklog/`). Never edit either by hand.
- Commits use conventional prefixes (`feat:`, `fix:`, `ci:`, `docs:`, `refactor:`,
  `chore:`, `test:`) with a crate scope where it helps (`feat(nid): ...`).
- The pre-push hook ([.githooks/pre-push](.githooks/pre-push)) mirrors CI's static gate.
  Enable it on a fresh clone:

  ```bash
  git config core.hooksPath .githooks
  cargo install cargo-audit cargo-machete cargo-deny cargo-nextest
  ```

## Where things live

| File | Holds |
|---|---|
| [README.md](README.md) | what orbistoun is, and quick start |
| [ACKNOWLEDGEMENTS.md](ACKNOWLEDGEMENTS.md) | reference-only credits |
| [docs/README.md](docs/README.md) | documentation hub |
| [docs/DECISIONS.md](docs/DECISIONS.md) | generated index of decisions |
| [docs/WORKLOG.md](docs/WORKLOG.md) | generated index of worklog entries |
| [docs/THE_LOOP.md](docs/THE_LOOP.md) | what one turn does, with a diagram |
| [docs/WORKFLOW.md](docs/WORKFLOW.md) | the commands that turn the loop |
| [docs/PROVENANCE.md](docs/PROVENANCE.md) | how a symbol name is shown to be ours |
| [docs/REFERENCES.md](docs/REFERENCES.md) | external documents relied on, what was taken, how it was checked |
| [docs/PROJECT_STATUS.md](docs/PROJECT_STATUS.md) | project status |
| [docs/ROADMAP.md](docs/ROADMAP.md) | intended order of work |
| [docs/BACKLOG.md](docs/BACKLOG.md) | considered work |
| [docs/SCOPE.md](docs/SCOPE.md) | what orbistoun is not |
| [docs/SYMBOLS.md](docs/SYMBOLS.md) | symbol database format and the hash suffix |
| [docs/ADDRESS_MAP.md](docs/ADDRESS_MAP.md) | every fixed base and its owner; check it before choosing an address, it is gated against the source (D513) |
| [docs/TESTING.md](docs/TESTING.md) | test strategy and the oracle problem |
