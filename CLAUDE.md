# CLAUDE.md

Working notes on **how** orbistoun is built and the principles to honour when
changing it. The README and `docs/` cover *what* it is and where it is going; this
file captures the constraints those decisions fit inside.

**Read [the OOPS conventions](https://github.com/project-oops/OOPS/blob/main/docs/CONVENTIONS.md) first.** Provenance, naming, honest failure, decision
logs, worklogs, stale facts and gates are shared across
[Orbistoun](https://github.com/project-oops/Orbistoun),
[obSCEne](https://github.com/project-oops/obSCEne),
[Prosperous](https://github.com/project-oops/Prosperous) and
[SELFish](https://github.com/project-oops/SELFish). This file holds what orbistoun adds, and how
it enforces what it shares.

## Mission, in one breath

A high-level hardware emulator in Rust. Guest x86-64 code runs natively; the work is
reimplementing the target operating system beneath it and translating GPU command
streams to Vulkan. No firmware, no keys, no derived code - so it stays distributable.

## Build principles

### 1. Provenance is a hard boundary

**The boundary itself is shared - see [OOPS conventions §1](https://github.com/project-oops/OOPS/blob/main/docs/CONVENTIONS.md#1-provenance-is-a-hard-boundary).**
What follows is how this repository *enforces* it, which is the part that is orbistoun's own.

The `provenance` CI job fails the build on any of it. Where a lawful reference exists, cite
it in a comment; the target kernel is FreeBSD-derived, so much of its C library has a
documented analogue and naming it costs nothing.

**Accounting is the mechanism**, and here it is a field rather than a habit. Every recorded
behaviour carries `known_by` - `published`, `measured`,
`guest-observed`, or `assumed` - and the vocabulary deliberately has no value meaning "I
already knew it". Every option names something that could contradict it, so writing a fact
down means committing to a checkable claim about where it came from. `orbistoun-cli learn`
refuses an entry that does not, and CI refuses a tree that does not.

`assumed` is not a failure state and most of this project is there. An assumption that is
written down can be counted, ranked, probed and retired; one written as though it were a
fact never will be.

**Other projects are reference-only, and get credited** in
[ACKNOWLEDGEMENTS.md](ACKNOWLEDGEMENTS.md), in the same change. The rule and its reasoning
are in the shared conventions; what matters here is that the file exists and is kept up.

### 2. Naming: no vendor trademarks in prose or in our own API

**The convention and its avoid/use table are shared - see
[OOPS conventions §2](https://github.com/project-oops/OOPS/blob/main/docs/CONVENTIONS.md#2-naming-no-vendor-brands-in-prose-or-in-our-own-api).**

The shared ABI exception, in its concrete form here: the literal symbol and library
name strings inside `guest_module!` declarations are **ABI identifiers**. The guest
imports by those exact names and the NID is computed from them, so renaming them stops
the tool working. They stay. Prose describing them does not have to repeat them.

Our own types carry no vendor prefix: `GuestError`, `GuestResult`, `guest_module!`,
`is_vendor_segment`.

### 3. Honest failure over plausible output

**The principle is shared - see
[OOPS conventions §3](https://github.com/project-oops/OOPS/blob/main/docs/CONVENTIONS.md#3-honest-failure-over-plausible-output).** A stub that returns
success is indistinguishable from working code until forty thousand frames later. Here that
binds three concrete things:

- `StubPolicy` defaults to `Unimplemented`, never `Ok`.
- `GuestError` placeholder codes deliberately avoid the high bit, so they can never
  be mistaken for real firmware values.
- `Container::imports` errors rather than returning an empty list - an empty list
  reads as "needs nothing", which is never true.

Never invent a constant, an error code, or an arity to make something compile
quietly. An explicit "not handled yet" is worth more than a wrong answer, and both
cost the same to write.

**And it applies to the tools, not only the emulator.** A guard, a verdict or a report
is as capable of plausible output as a stub is, and five things did exactly that in one
session: a gate that checked a different field from the one it claimed, an audit that
returned before its own ceiling comparison, a readable window a page out that reported
pointers as counts, a progress verdict naming a cause it had not measured, and a
conclusion drawn because a fault moved. Every one *reported more than its measurement
supported*, which is the same failure the principle already forbids one level down.

Three rules fall out of it, and each is cheap:

- **A guard is not finished until somebody has made it fail.** Every one of the first
  three would have been caught by writing the negative test. A guard nobody has watched
  reject something is a guard nobody knows anything about.
- **A message naming a cause must come from the branch that determined it.** `Further`
  fires for two different reasons and said only one of them.
- **An intervention that moves a wall is not a diagnosis.** A diagnostic that *changes*
  the program - a poke, a poison, a reservation - can buy progress with a wrong answer.
  It needs a second observation, of a different kind, saying what the guest did with it.
  `orbistoun-env` records which diagnostics intervene and the run report says so
  (D224, D226, D227).

Counting successes is not checking for failures: assert on the failure, never on the
count of passes.

### 4. Unsafe is contained, documented, and rare

Guest code runs in this process, so `unsafe` is unavoidable. Undocumented `unsafe`
is not. Enforced by lints, not vibes: `undocumented_unsafe_blocks` and
`multiple_unsafe_ops_per_block` are **deny**, and `unsafe_op_in_unsafe_fn` is deny.

- Every `unsafe` block carries a `// SAFETY:` comment stating the invariant that
  makes it sound, not what the code does.
- Guest memory access is confined to `orbistoun-mem`. Everything above it takes
  safe, checked accessors. If a subsystem crate needs a raw pointer, the abstraction
  is in the wrong place.
- Parsing never uses `unsafe`. `orbistoun-elf` handles hostile bytes and contains
  zero - `zerocopy` validates size and alignment first. Keep it that way.

### 5. Rules and policy live in data, not code

Stub returns, symbol databases, and the NID hash suffix are all runtime inputs.
Nothing about a specific title or a specific firmware version is compiled in. The
test: if answering "what does this function return?" requires a rebuild, it is in the
wrong place.

This is what makes the bisection workflow cheap - edit a TOML, relaunch, observe.
That loop is the only oracle available for most functions, so protect it.

### 6. One dependency spine, built in order

`core` → `elf` → `nid` → `mem` → `hle` → `loader`, then subsystems.

Subsystem shims are never reached until a guest has loaded, allocated, and spawned
threads. Writing the audio shim before the address space works produces code that
cannot be exercised, so it cannot be trusted. Resist it however tempting audio and
video look - they are the visible ones, not the next ones.

### 7. Interception is linking, not hooking

A guest imports by NID; the loader resolves it and writes the address into the guest
relocation slot. There is no instrumentation pass and there must never be one. This
is what makes the full import list available statically, before execution.

If you find yourself adding a hook, patch, or trampoline injection step, the
resolution path is being worked around rather than used.

### 8. Tests pin behaviour, not implementation

The high-value targets are the pure ones with concrete contracts: NID hashing,
address-space validation, policy resolution, container parsing. These are written
test-first and each test states the property it protects in a comment.

Note the pattern in `orbistoun-mem`: validation is separated from mapping precisely
so the ABI rules are testable without touching the host address space. Prefer that
shape - a pure decision function plus a thin effectful wrapper - wherever it fits.

### 9. Traces are binary, indexed, and attributed

Text logging does not survive emulator call volume, and "which function" is the
wrong question - "which call site" is the right one. Every event carries a global
monotonic sequence number and the guest return address. Recording must not allocate;
a sink that blocks a guest thread has changed the program it observes.

### 10. Greenfield: no legacy, no compatibility shims

Shared, and stated once in [OOPS conventions §7](https://github.com/project-oops/OOPS/blob/main/docs/CONVENTIONS.md#7-greenfield-no-legacy-no-compatibility-shims).
Nothing has shipped; edit the original and wipe the file.

## Where the oracle comes from

Most of this project is guessing at undocumented semantics, so it is worth being
precise about the four places real ground truth exists. Prefer them, in order:

1. **FreeBSD source** - much of the target C library is POSIX with vendor naming.
   Lawful, citable, and the strongest reference available.
2. **Framebuffer diffing** - for the GPU layer, render and compare numerically. The
   only cheap, mechanical correctness signal in the whole codebase.
3. **The guest itself** - a 1-bit oracle per call site: return `Ok`, does it proceed?
   Expensive per query (a boot), so use a prior to pick what to try.
4. **Instruction test suites** - total ground truth, but only for retro targets.
   Useful for validating tooling before pointing it at something unverifiable.

If a change cannot be justified from one of these, say so in the commit message.

### 11. No urgency; take the highest-payoff path

This is a personal proof of concept. Nothing is being delivered to anyone, there is
no deadline, and finishing sooner is worth nothing. Where two paths differ, take the
one with the better end state.

**A shortcut that constrains the design is never worth the time it saves.** Stated as
a principle because "simplest thing that works" is the default drift of a long
unattended run - see D032, where the first recommendation was the shortcut and
reconsidering under this rule reversed it. (D028)

### 12. Contracts, not hardcoding - abstracted at guest semantics

One graphics backend exists today; adding another must never require surgery. Same
for audio, input, and filesystem. Any string crossing a boundary is a named constant
declared once.

**Abstract at the level of what the guest asks for, not what the host API provides.**
A backend trait designed by looking at Vulkan carries descriptor sets and render
passes into the contract, and then a second backend fits badly anyway. Each backend
maps guest semantics onto its own primitives.

**The test for whether a seam is premature:** if it pays off only hypothetically, it
is speculation; if it buys testability or swappability *now*, it is structural.

**Enforce with crate boundaries, not vigilance.** `orbistoun-gpu` has no dependency
on `ash`, so host-API leakage is impossible rather than discouraged, and `cargo`
polices it instead of code review.

It deliberately stops at two places: there is **no execution-backend abstraction**
(native x86-64 execution is the architecture, and no second implementation will ever
exist) and **no container-format plugin layer**. Both pay no rent, now or later.
(D029, D030, D031)

### 13. Shims hold no logic

The crates are the emulator. `orbistoun-cli`, `orbistoun-gui`, and worker mode are
interaction shims over them - none is privileged, and none contains behaviour the
others lack. Shared orchestration lives in `orbistoun-service`; the shim-to-worker
protocol is serialisable data in `orbistoun-proto`, defined separately from its
transport.

If a shim starts holding logic, the other two are already drifting. (D033, D034,
D035)

## Start here

If you are picking this up cold, do this before reading anything else:

```bash
./bin/orbistoun doctor --fix  # is this machine ready; --fix installs what is missing
./bin/orbistoun check         # is the tree sound
./bin/orbistoun run <title>   # one turn of the actual work
```

That last command is the project. It resolves a title, refreshes symbol names if they
are stale, runs the guest, reports what it asked for, and says whether it got **further
than last time**. Everything else here exists to make that one command informative.

**The loop, in full:**

1. `./bin/orbistoun run <title>` - see where the guest dies and what it wanted
2. Read the ranked findings. Each one ends in an arrow saying what to do about it
3. Either **implement** it (a subsystem crate) or **name** it (extend
   `crates/orbistoun-names/data/vendor.toml`) if it is still a bare hash
4. Run again. The `progress` block says `FURTHER`, `same`, or `BACK`
5. `orbistoun-cli learn` anything the turn established, before it exists only in a terminal
6. `./bin/orbistoun check` before calling anything done

`FURTHER` means the guest executed code it could not reach before. That is the only
measure of progress this project has, and it is the one to optimise.

**[docs/THE_LOOP.md](docs/THE_LOOP.md) is the full picture** - every step, a diagram, and
which parts still need a person. [docs/WORKFLOW.md](docs/WORKFLOW.md) is the command
reference beside it.

**Check the sibling repositories before starting an investigation.** obSCEne
([repository](https://github.com/project-oops/obSCEne)) is a conformance probe for the same platform, and its `docs/` already
answer things this project would otherwise work out from scratch - `HANDOVER-ORBISTOUN.md`
documented the vendor dynamic tags before an afternoon here rediscovered them
independently. Reading a sibling project's *own notes* is ordinary engineering and costs
nothing; the provenance boundary in principle 1 is about other people's **source**, not
about our own documentation in another directory.

The same applies in reverse: findings worth having on both sides belong in a document, not
in one repository's decision log.

**Do not start by reading the decision log.** It is over two hundred entries and it is a
reference, not an introduction - consult it when a choice seems arbitrary, because the reasoning is
almost always in there.

## Working sessions, and surviving compaction

This project is built in long unattended runs. Context gets compacted; conversation
does not persist. **Anything that exists only in a conversation is already lost.**

**At the start of every session, read [docs/DECISIONS.md](docs/DECISIONS.md) and
[docs/WORKLOG.md](docs/WORKLOG.md).** They are the durable memory. This file holds
the principles; those two hold the history and the state.

**While working:**

- Every non-obvious choice gets a numbered entry in `DECISIONS.md` **as it is made**,
  not retrospectively. Include the reasoning, not just the choice - the reasoning is
  what stops it being re-litigated.
- A choice made without input is status **`assumed`**, and its number goes in that
  file's *Needs review* index. Assume freely and keep moving; do not stall an
  unattended run waiting for input that cannot arrive. Just record it.
- **Assume freely on implementation; flag a new concept.** New crates, splitting
  things up, file layout, naming, structure - all expected, no need to ask. What
  warrants stopping is a *concept* not already in the decision log: a mechanism, a
  user-visible behaviour, or a subsystem nobody agreed to. Adding a crate is not a
  new concept; adding a plugin system is.
- Append to `WORKLOG.md` at the end of every **completed unit of work**, not at the
  end of a session - a session may not end cleanly. Record surprises especially:
  they are what a fresh context cannot re-derive.
- Run `./bin/orbistoun check` before logging a unit as done.

**When in doubt about whether something is worth writing down: write it down.** The
cost of a redundant log entry is a few lines. The cost of a lost decision is
rediscovering it by making the same mistake.

## Active conventions

- **Conventional commits** - `feat:` / `fix:` / `ci:` / `docs:` / `refactor:` /
  `chore:` / `test:`, with a crate scope where it helps (`feat(nid): …`).
- **Lints are a workspace table**, not just CI flags, so rust-analyzer applies them
  while you type. CI adds `-D warnings` on top.
- **Pre-push hook** ([.githooks/pre-push](.githooks/pre-push)) mirrors CI's static
  gate. Enable on a fresh clone:
  ```bash
  git config core.hooksPath .githooks
  cargo install cargo-audit cargo-machete cargo-deny cargo-nextest
  ```

## Where things live

- [README.md](README.md) - public pitch + quick start.
- [ACKNOWLEDGEMENTS.md](ACKNOWLEDGEMENTS.md) - reference-only credit list.
- [docs/DECISIONS.md](docs/DECISIONS.md) - every decision, numbered, with reasoning.
- [docs/WORKLOG.md](docs/WORKLOG.md) - what was done, in order, plus surprises.
- [docs/README.md](docs/README.md) - documentation hub.
- [docs/THE_LOOP.md](docs/THE_LOOP.md) - what one turn does, start to finish, with a diagram.
- [docs/WORKFLOW.md](docs/WORKFLOW.md) - the commands that turn it, and their cadence.
- [docs/PROVENANCE.md](docs/PROVENANCE.md) - how a symbol name is shown to be ours.
- [docs/REFERENCES.md](docs/REFERENCES.md) - every external document relied on, what was
  taken from each, and how it was checked.
- [docs/PROJECT_STATUS.md](docs/PROJECT_STATUS.md) - what works today.
- [docs/ROADMAP.md](docs/ROADMAP.md) - committed next.
- [docs/BACKLOG.md](docs/BACKLOG.md) - everything considered, loosely ranked.
- [docs/SCOPE.md](docs/SCOPE.md) - what orbistoun deliberately is not.
- [docs/SYMBOLS.md](docs/SYMBOLS.md) - symbol database format and the hash suffix.
- [docs/TESTING.md](docs/TESTING.md) - the test strategy and the oracle problem.
