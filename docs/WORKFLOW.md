# Workflow

The commands that turn the loop: what to run, with which flags, and when.
[THE_LOOP.md](THE_LOOP.md) describes what a turn does, who does each step, and how to read
its output.

`./bin/orbistoun` is the development driver: it builds, resolves title ids and passes the
symbol database. `orbistoun-cli` is the tool itself (`./target/release/orbistoun-cli`, or
`./bin/orbistoun cli <args>` for the debug build). `./bin/orbistoun --help` lists every verb;
`orbistoun-cli --help` lists every subcommand.

## Setup

```bash
./bin/orbistoun doctor --fix   # what is missing, and install it
./bin/orbistoun check          # confirm the tree is sound
orbistoun-cli paths            # where the title library, config.toml and outputs are
./bin/orbistoun run <title>    # first turn
```

`doctor` runs automatically before any verb that needs a toolchain, so a missing requirement
surfaces as one line rather than as a build error. `--fix` installs the optional tools and
enables the pre-push hook; it does not install a Rust toolchain, which is a machine-wide
decision left to the user.

A title is a directory holding an `eboot.bin` in the title library that `orbistoun-cli paths`
prints. `orbistoun-cli corpus sync` fills the library from the pinned sources in
`corpus/sources.toml`.

## One title

```bash
./bin/orbistoun run <title-id> [-- <extra args>]
```

Resolves the title id to a module (a staged title under the library's `data/homebrew` first,
then the library; a path also works), builds the release binary, refreshes names if they are
stale, runs the guest, and prints the worklist for it. With no argument it lists the titles
available. `ORBISTOUN_LIMIT` sets the time limit in seconds (default 20). Arguments after `--`
go to `orbistoun-cli run`.

Names are rebuilt when the grammar, the standard word list or the module is newer than the
symbol database. The rebuild searches the whole corpus in one sweep, whose cost does not
depend on how many hashes it looks for (D213).

`orbistoun-cli run` takes:

| Flag | Default | Meaning |
|---|---|---|
| `--limit <s>` | 20 | Seconds of guest execution; `0` removes the limit |
| `--calls <n>` | 20000000 | Imports the guest may call; the deterministic limit, `0` removes it |
| `--profile <name>` | `shell.toml` | Present a named hardware profile, e.g. `prospero-cex-12.40` |
| `--input <script>` | none | Play this pad script on player 1 (D721) |
| `--staged` | off | Run a loose build as a staged title with a writable `/app0` (D722) |
| `--relink` | off | Replace the title's stored link plan with this run's and print what differed (D724) |

## One turn, unattended

```bash
./bin/orbistoun turn <title-id> [--record | --apply | --verify <file>]
```

Runs `orbistoun-cli turn` with the same resolution and symbol database as `run`: the guest,
the ranked findings, and every mechanical step the findings call for (see
[THE_LOOP.md](THE_LOOP.md#step-17-the-dispatcher)).

| Flag | Effect |
|---|---|
| `--record` | Print what the turn established as a `learn` command |
| `--apply` | Write what the turn measured into `learned.toml` beside `config.toml`. Every entry a person wrote wins over it; deleting the file undoes it |
| `--verify <file>` | Check a submitted learned file against what this machine measures |

## Every title

```bash
./bin/orbistoun sweep
```

Refreshes names, runs every module in the library, and ends in a ranked list of what all of
them called:

```
         CALLS  SHARE  MODULES  IMPORT
      87621502  99.9%        1  libkernel::sceKernelDirectMemoryQuery
          1433   0.0%        4  libc::__cxa_atexit
           474   0.0%        4  libc::memset
```

The list is ranked by calls, not by modules. An enormous count is a guest waiting in a loop
for an answer - a wall rather than a feature - and the ordering makes that obvious.

```bash
orbistoun-cli worklist [--top N]    # totals across every trace on disk; re-runs nothing
orbistoun-cli worklist --static-gap # rank static import lists by where an answer can come from
```

## Inspecting

```bash
orbistoun-cli knows [pattern]             # what is recorded about a function, and on what it rests
orbistoun-cli questions [--top N]         # every open assumption, ranked by call frequency
orbistoun-cli questions --premises        # grouped by the premise entries share
orbistoun-cli questions --json            # for a probe or an agent
orbistoun-cli imports <module>            # what a module imports, without running it
orbistoun-cli imports <module> --own      # the modules the title ships that answer its imports
orbistoun-cli report <module>             # survey a module, persist a report, show the delta
orbistoun-cli link <module> [--relink]    # link a title, store its plan, print modules, writes, digest
orbistoun-cli compat list                 # how far each title got, furthest first
```

`knows` says whether the current behaviour of a function is `published`, `measured`,
`guest-observed` or `assumed`, and what nobody has established. Changing something that rests
on a citation is a different act from changing a guess, so check it before editing an
implementation.

## Recording what a turn produced

```bash
orbistoun-cli learn <function> --library <lib> --known <how> [--purpose ... --edge ... --assumes ...]
orbistoun-cli compat record <module> [--note "..."] [--force]
orbistoun-cli submit export [--out submission]
orbistoun-cli submit check <directory>
```

`learn` appends to the knowledge files under `crates/orbistoun-hle/data/knowledge/` and
refuses an entry that does not say where it came from. It is never automatic: a finding
recorded by hand is one somebody decided was true.

`compat record` transcribes the last trace of a title into `compat/<title>.toml`. A run
measuring the emulator as it stands updates the `[status]` slot; a run helped by a loosened
default or by `learned.toml` answers updates `[experiment]`. Each slot is compared only with
itself, so no run is refused for its policy (D312). `--force` replaces a better entry within a
slot. The run prompts for this after it moves.

`submit export` collects the `learned.toml` measurements and both compat slots into one
directory with a manifest naming the build. It refuses to write an empty bundle.
`submit check` re-derives what a bundle claims rather than trusting it: agreement is silent, a
claim this machine never measured is reported as unmeasured rather than as a contradiction,
and files are counted rather than read off the manifest (D315). A `patches/` directory with a
`patches.toml` describing each diff travels in the bundle and is reported apart from the
claims; a patch is inert until a person reads it, runs the gate and merges it.

## Names

```bash
./bin/orbistoun names                         # regenerate symbols/ from the library, then audit
orbistoun-cli names <dir> --out symbols/generated.json --wanted symbols/wanted.txt --from-trace
orbistoun-cli audit symbols/generated.json    # re-derive every name from this repository's inputs
orbistoun-cli audit symbols/generated.json --verify-harvest   # re-read the modules behind static records
orbistoun-cli nid <name>...                   # the import hash of a name
orbistoun-cli harvest <freebsd-src>           # rebuild the standard-library word list
./bin/orbistoun suggest [rounds]              # ask a local model for vocabulary
```

`names` on a directory is one search over the whole corpus, and it is the only form that finds
a name in one title's strings explaining another title's import (D213). `--from-trace` adds
strings a previous run captured from guest memory. `./bin/orbistoun names` also runs
`audit --repair --verify-harvest`, because every learned word renumbers the candidates a
generated record cites.

`ORBISTOUN_PROBE_REPORTS` names a directory of obSCEne reports. `./bin/orbistoun names` reads
every `*.obs.log` under it and passes each as `--from-report`: hashes the platform exports, as
targets to name, never as names. The variable is a path rather than a script setting
because the reports live in another repository, and a path written here would make a build
dependency between the two. It is not listed by `orbistoun-cli env`, because only the shell
driver reads it.

```bash
ORBISTOUN_PROBE_REPORTS=<directory of reports> ./bin/orbistoun names
```

`suggest` runs `orbistoun-suggest` (three rounds per grammar position by default) and writes
the words the hash confirms to `symbols/proposed-*.txt`. Promoting one is manual: put a noun
in `object` or a suffix in `tail` of `crates/orbistoun-names/data/vendor.toml`, then run
`./bin/orbistoun names`. Before asking for words,
`cargo test -p orbistoun-propose --release --test shapes -- --nocapture` says whether the
unnamed imports are short of vocabulary or short of shapes.

## Probe transcripts

```bash
orbistoun-cli probe <transcript> --device <name> [--firmware <version>] [--is-target]
orbistoun-cli probe <transcript> --as-knowledge          # print what it established as knowledge entries
orbistoun-cli probe <local-transcript> --against <hardware-transcript>
```

`probe` grades an obSCEne transcript by the machine the operator says it ran on. `--against`
compares a run under orbistoun with the same probe's transcript from the hardware and reports
the checks that disagree. Sending and supervising the probe is Prosperous's job (see
[THE_LOOP.md](THE_LOOP.md#probing-on-the-hardware)).

## Settings and the environment

| | Where | What belongs in it |
|---|---|---|
| Settings | `<data>/config.toml` | How the emulator behaves: entry presentation, thread placement, memory behaviour, the library folder, what unimplemented functions answer. Persistent; the file to edit to bisect a stub |
| The environment | variables listed by `orbistoun-cli env` | The two settings that decide where `config.toml` is, and every diagnostic |

`config.toml` is composed of settings owned by the loader, the kernel and the HLE layer, so it
sits high in the dependency spine. The environment registry sits at the bottom, because
`orbistoun-paths` needs it to find the data root (D221).

## Diagnostics

```bash
orbistoun-cli env
```

`env` prints every variable orbistoun reads, what each is for, an example, the crate that
reads it and its current value, from the one registry in `orbistoun-env`. This document does
not copy the list.

- A diagnostic is not a setting. It answers one question once, which is why it lives in the
  environment and not in `config.toml` (D221).
- Every diagnostic in effect is recorded in the run's conditions, so a verdict taken under one
  is never compared with an ordinary run (D181).
- An import is matched by name or by any part of its label, so an unnamed function is
  addressed by its hash: `ORBISTOUN_DUMP=0x6abac2f3dc6f8cee`.
- A request that matches nothing says so, and a variable that is nearly a known name
  (`ORBISTOUN_STACK_FIL`) is reported as unrecognised.

```bash
# Force argument dumps for functions that are already implemented.
ORBISTOUN_DUMP=memalign,malloc ./bin/orbistoun run <title>
# What is the wall handed, and does planting a value move the fault?
ORBISTOUN_WRITE=0x6abac2f3dc6f8cee:0:0x11000000 ./bin/orbistoun run <title>
```

Dumps are attached to unimplemented calls by default; `ORBISTOUN_DUMP` forces them where the
implementation is the suspect, scalars included (D179). Only the stack is writable by a forced
write: the image's runs are protected after relocation.

`ORBISTOUN_WATCH` and `ORBISTOUN_WATCHPOINT` compose. The first copies a region before the run
and diffs it afterwards, naming the words nobody wrote; the second arms an x86 debug register
and reports which instruction touched an address, how often, and what it saw (D276).

```bash
# 1. Which words in this structure did nobody write?
ORBISTOUN_WATCH=0x4000019e9c00+0x80 ./bin/orbistoun run <title>
# 2. Who reads the one that stayed zero?
ORBISTOUN_WATCHPOINT=0x4000019e9cb0:rw ./bin/orbistoun run <title>
```

`ORBISTOUN_WATCHPOINT` takes `<addr>[+len][:w|rw]`: up to four, each of one, two, four or eight
bytes aligned to its length. x86 has no read-only encoding, so `rw` also traps writes. A data
breakpoint fires after the access completes, so each line reports `after the access at` the
next instruction; naming the instruction itself would mean disassembling a vendor binary
(D276).

## Outputs

| Artifact | Where | Written by |
|---|---|---|
| Call traces | `<data>/traces/*.json` | every run, on every outcome |
| Run reports | `<data>/reports/` | `orbistoun-cli report` |
| Learned answers | `<data>/learned.toml` | `orbistoun-cli turn --apply` |
| Names worked out | `symbols/generated.json` | `./bin/orbistoun names`, accumulating |
| Hashes still unnamed | `symbols/wanted.txt` | the same |
| What a title reached | `compat/<title>.toml` | `orbistoun-cli compat record` |
| What this machine can contribute | `submission/` | `orbistoun-cli submit export` |
| What is known, and not | `crates/orbistoun-hle/data/knowledge/` | `orbistoun-cli learn` |
| Window captures | `<data>/screenshots/*.png` | the GUI toolbar's capture button (D162) |

`<data>` is the platform data directory, or the binary's own directory in portable mode;
`orbistoun-cli paths` prints it. Traces are keyed by module, and `worklist` totals across all
of them.

## Run limits

A guest whose imports all answer "unimplemented" can wait forever on a function it calls
millions of times without faulting. Killing it from outside loses the trace, so the worker
stops it itself, writes the trace, and exits with a status saying which limit fired (D238):
the time limit (`--limit`) or the call budget (`--calls`). The call budget is the deterministic
one - two runs of one build stop at the same call - and the clock is a backstop for a guest
that stops calling imports.

## Cadence

| Run | When |
|---|---|
| `./bin/orbistoun run <title>` | Constantly; this is the debug loop |
| `./bin/orbistoun turn <title>` | When the top finding is one the dispatcher can sweep |
| `./bin/orbistoun sweep` | After any change that could move a guest further |
| `./bin/orbistoun check` | Before any unit of work is done |
| `./bin/orbistoun names` | Included in `sweep`; separately after extending the vocabulary |
| `orbistoun-cli worklist` | Any time; it reads persisted traces |
| `orbistoun-cli questions` | Before implementing a function |
| `orbistoun-cli learn` | Whenever a turn established something |
| `orbistoun-cli compat record` | After a run that moved; the run prompts for it |
| `orbistoun-cli harvest <freebsd-src>` | For a larger standard-library word list |
| `./bin/orbistoun symbols-audit` | Included in `check` and in CI |

## Without a title

With nothing in the library, `sweep`, `worklist`, `questions` and `compat list` still run. The
name generator produces candidates, but confirming one needs a real import table to collide
against; [PROVENANCE.md](PROVENANCE.md) says what that implies.
