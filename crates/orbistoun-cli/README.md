# orbistoun-cli

The `orbistoun` binary. One of three interaction shims over the crates - it holds no
behaviour the GUI and worker mode lack (principle 13).

Twenty commands, grouped by what they need to exist before they work.

## Needs nothing but the tree

| Command | What it does |
|---------|--------------|
| `symbols` | Every library and function orbistoun declares |
| `knows` | What is recorded about those functions, and what each claim rests on |
| `questions` | Everything written down that this project does **not** know, ranked by how often guests call it |
| `policy` | Emit a default stub-policy file to edit |
| `paths` | Where orbistoun reads and writes, and whether it is in portable mode |
| `learn` | Record something worked out about a guest function |
| `audit` | Re-derive every name in a symbol database from this repository's own inputs |
| `harvest` | Rebuild the standard-library word list from a FreeBSD source tree |

## Needs a guest module on disk

| Command | What it does |
|---------|--------------|
| `inspect` | A container's structure, without executing or fully parsing it |
| `imports` | What the module needs, without executing it |
| `verify` | How much of that import list a symbol database can name |
| `names` | Search generated candidates for ones that hash to the unnamed imports |
| `load` | Reserve the address space it demands, without executing it |
| `run` | Execute the guest in a worker process, then report |
| `report` | Survey, persist a run report, and show the delta from last time |

## Needs runs to have happened

| Command | What it does |
|---------|--------------|
| `worklist` | Rank what to implement next, totalled across every trace on disk |
| `compat list` / `compat record` | How far each title got; written from a trace, not by hand |

## Needs hardware, or a transcript from some

| Command | What it does |
|---------|--------------|
| `session` | Drive a live session against a listening conformance probe |
| `probe` | Read a transcript or corpus and report what it establishes |

## Needs shader binaries

| Command | What it does |
|---------|--------------|
| `shaders` | Analyse a directory of shader binaries and rank what blocks translation |

## Examples

```bash
cargo run -p orbistoun-cli -- symbols --filter AudioOut
cargo run -p orbistoun-cli -- policy > stubs.toml
cargo run -p orbistoun-cli -- questions --top 20
cargo run -p orbistoun-cli -- worklist --top 40
```

Most day-to-day use goes through `./bin/orbistoun run <title>` instead, which drives
`names` and `run` and `worklist` in the right order. See [docs/THE_LOOP.md](../../docs/THE_LOOP.md).

**`--suffix-hex`** overrides the NID hash suffix and is rarely needed - the shipped
value verifies itself against published C library names. See `docs/SYMBOLS.md`.

**`imports` reports an honest error** when a container cannot be parsed. An empty
import list would read as "this title needs nothing", which is never true.

**Design note.** `modules()` in `crates/orbistoun-service/src/symbols.rs` is the one place that
knows the full module set, so wiring up a new subsystem crate is exactly one line there
plus its `guest_module!` declaration. It used to be a `build_registry` function in this
crate; a shim holding that list was the drift principle 13 exists to stop.

**Status:** every command above works. `session` has never been run against real
hardware, because there is none yet - `probe` reads its transcripts and is exercised
against recorded ones.
