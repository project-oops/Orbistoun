# orbistoun-cli

The `orbistoun-cli` binary: one of three interaction shims over the crates, beside the GUI
and worker mode. It holds no behaviour the other shims lack; shared operations live in
[orbistoun-service](../orbistoun-service/) (CLAUDE.md, *Shims hold no logic*).

`orbistoun-cli --help` and `orbistoun-cli <command> --help` are the full reference. The
commands group by what they need.

## Needs nothing but the tree

| Command | What it does |
|---------|--------------|
| `symbols` | List every library and function orbistoun declares |
| `knows` | Print what is recorded about guest functions, and what each claim rests on |
| `questions` | Rank everything recorded as unknown by how often guests call it |
| `learn` | Record something worked out about a guest function in the knowledge file |
| `policy` | Print a default stub-policy file to edit |
| `paths` | Print where orbistoun reads and writes, and whether it is in portable mode |
| `env` | List every environment variable orbistoun reads, and what is set |
| `nid` | Compute the import hash for one or more names |
| `audit` | Re-derive every name in a symbol database from this repository's own inputs |
| `harvest` | Rebuild the standard-library word list from a FreeBSD source tree |
| `status` | Emit the generated numbers block for the documentation, or check it for drift |
| `firmware` | Show how the firmware skeleton lays libkernel out, and where a stub overruns its neighbour |

## Needs a guest module on disk

| Command | What it does |
|---------|--------------|
| `inspect` | Report a container's structure without executing or fully parsing it |
| `imports` | Report what a module imports, without executing it |
| `exports` | Report what a module exports, as hashes |
| `verify` | Report how much of an import list a symbol database can name |
| `names` | Search generated candidates for names that hash to the unnamed imports |
| `load` | Reserve the address space a module demands, without executing it |
| `run` | Execute a guest in a worker process, then report |
| `report` | Survey a module, persist a run report, and show the delta from the last one |
| `handoff` | Find which process-handoff fields a guest's runtime reads |
| `turn` | Turn the loop once against a title, taking every mechanical step and stopping at the ones that need a person |

## Needs runs to have happened

| Command | What it does |
|---------|--------------|
| `worklist` | Rank what to implement next, totalled across every trace on disk |
| `compat list` / `record` / `markdown` | Read, record (from a trace, never by hand) and render the per-title compatibility record |
| `corpus list` / `sync` / `run` | Show the corpus manifest, fetch its guests pinned by hash, and run and record each |
| `submit export` / `check` | Gather this machine's measurements and title results, or re-derive and compare a received bundle |

## Needs hardware, or a transcript from it

| Command | What it does |
|---------|--------------|
| `session` | Drive a live session against a listening conformance probe and record the transcript |
| `ask` | Ask a live probe one question and print what it answers |
| `probe` | Read a transcript or corpus and report what it establishes |
| `serve` | Answer the conformance probe's command protocol, so one driver can drive either |

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

Day-to-day use goes through `./bin/orbistoun run <title>`, which drives `names`, `run` and
`worklist` in order. See [docs/THE_LOOP.md](../../docs/THE_LOOP.md).

`--suffix-hex` overrides the NID hash suffix. The shipped value verifies itself against
published C library names, so it is rarely needed. See [docs/SYMBOLS.md](../../docs/SYMBOLS.md).

`imports` reports an error when a container cannot be parsed. An empty import list would read
as "this title needs nothing", which is never true.

## Adding a subsystem

`modules()` in `crates/orbistoun-service/src/symbols.rs` is the one place that knows the full
module set. Wiring a new subsystem crate is one line there plus its `guest_module!`
declaration; nothing in this crate changes.
