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
orbistoun-cli link <module> --native <dir> [--name <n>]  # also write an orbistoun-aot build into <dir>
orbistoun-cli compat list                 # how far each title got, furthest first
```

`knows` says whether the current behaviour of a function is `published`, `measured`,
`guest-observed` or `assumed`, and what nobody has established. Changing something that rests
on a citation is a different act from changing a guess, so check it before editing an
implementation.

## Profiling

```bash
orbistoun-cli perf --install-tracy           # fetch the Tracy tools this build's client speaks, once
orbistoun-cli perf <module> [--seconds N]    # record a run with Tracy; frame times, zones by self and total time
orbistoun-cli perf <module> --open           # ... and open the recording in the Tracy viewer
orbistoun-cli bench [title...] [--seconds N] # every title in turn, costs ranked across them, against the last bench
orbistoun-cli bench --at HEAD                # ... of a revision's clean build, not this tree's
```

A measured run records 30 seconds, or a title's own `perf_seconds` setting where it needs longer to
pass its start; `--seconds` sets every title's. Each title is compared with its own last
measurement, from whichever benchmark took it.

```bash
orbistoun-cli ab <module> --baseline <exe>   # another build and this one, alternately; fps and the zones that moved
orbistoun-cli ab <module> --against HEAD     # ... against a revision, built with Tracy from a clean export and kept
```

A recording and its report go in the title's `perf` directory, with the run's log and the guest's
own output beside them (`<stamp>.log`, the terminal showing only the report), and a benchmark
under the data root's `bench`; neither touches a compat record. Ranking by self time names what to
make faster, and the frame-time percentiles name what a player feels, which an average hides. A
benchmark ranks each zone by the share of its title's run it took, every title weighing the same,
so an idle title's few milliseconds of presenting do not read as all of its time. See
[profiling with Tracy](features/graphics.md#profiling-with-tracy) and D831.

A round of making a title faster goes:

1. `perf` the title, long enough to pass its start: a homebrew title may show a controls card for
   ten seconds and build its fonts before it draws anything, so 30 seconds rather than 15. Read
   the run log's "its threads asked to sleep" first: a title that sleeps a fixed time a frame is
   paced by itself, and its frame rate is the hardware's too.
2. Read the report's "waiting on the device" first: a run that spends a third of its time
   waiting for the device is bound by the device's work, and a wait removed from one span
   reappears in another (`DEVICE_WAITS` beside the span table). Then take the largest *self* time. A span that waits on another thread's - `execute hand-off`, a
   guest thread waiting on the device thread - keeps only the hand-off as its own time; the work
   is ranked under the device thread's zones. A new such pair goes in `WAITS_ON` beside the span
   table in `crates/orbistoun-gpu/src/perf.rs`.
   A "caches emptied whole" line names caches that filled and were let go: about once a frame is
   a working set larger than the cache, and every entry it kept is worked out again - let the
   oldest go instead, or keep more. A cache that empties itself whole when full gets an
   `emptied:` span and its place in `EMPTIED` beside the span table.
3. Where that zone has nothing finer, add a span inside it - one line in the span table of
   `crates/orbistoun-gpu/src/perf.rs` and a `perf::span` around the work - and record again,
   rather than timing it by hand. A span kept is the next round's answer.
   A span that builds a frame-sized buffer and costs about a quarter of a microsecond per 4 KB
   page of it is paying for a fresh allocation, which a host gives a megabyte at a time page by
   page: keep the storage from one frame to the next rather than making it again.
4. Where the spans run out, `perf --sample` samples the guest's main thread and the device thread
   (`ORBISTOUN_PROFILE`), naming host functions, and adds each thread's last sample to the report.
   Read a sampled name as "inside or near": a sample is named by the closest symbol, and a release
   build inlines a function's callees into it, so their time - and a system call they make - is
   counted under its name. It also watches the threads that submit and carry out draws, which in a
   title drawing from a thread of its own are not the thread that started it.
5. Change one thing and compare it with `ab --against HEAD` - the commit before, built cleanly: a run
   on the host clock varies by a few frames a second between runs of one build, and other work on
   the machine moves it more, so `ab` runs the two builds alternately for several rounds, after an
   uncounted warm-up run of each, since a fresh build's first run fills its caches. Judge by
   the zone you changed as well as by the frame rate. `--check-frames` adds whether the change
   draws what the baseline does: on the logical clock, the baseline twice and the change once,
   compared at every flip the baseline repeats. A large gain is suspect until it passes - a
   prototype that ran PPSA02664 six times faster changed how its loop ran.

`perf`, `ab` and `bench` wait up to a minute before each measured run for anything else using the
GPU - another session's run on the same device slows the title's device time, not its host time,
and reads as a slower build. A run that could not wait it out says so at the top of its report.
A busy CPU is not waited for, since a virtual machine beside the run may last all day, but a run
beside one says so the same way: a title bound by its guest thread loses a quarter of its frame
rate beside ten busy cores, so compare its frame rate only with a run made beside the same load -
`ab`, which alternates the builds, does.
Change the build in a `git archive` export rather than the shared working tree: another session's
uncommitted edits there go into whatever is built from it. `ab --against HEAD --with <file>...`
does that: it measures `HEAD` against `HEAD` with only the named files taken from the working tree,
both built from exports. A file that also holds another session's hunks brings them with it - look
at its diff first, or name it `<place>=<copy>`: the candidate is written to a copy outside the tree
(`git show HEAD:<place>` and the change) and laid at its place, the working tree never touched.

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

`compat record` transcribes the last trace of a title into `compat/<title>/report.toml`. A run
measuring the emulator as it stands updates the `[status]` slot; a run helped by a loosened
default or by `learned.toml` answers updates `[experiment]`. Each slot is compared only with
itself, so no run is refused for its policy (D312). `--force` replaces a better entry within a
slot. The run prompts for this after it moves.

```bash
orbistoun-cli compat compare <title> --reference <directory> [--tolerance <steps>] [--record <source>]
```

`compat compare` is framebuffer diffing against the console. It runs the title as its record's
`[reproduce]` section says, keeping the frames drawn before exactly the flips the reference
directory holds, and compares each with the console's frame of the same flip. References are
`flip-<n>-<w>x<h>.rgba` or `flip-<n>.png`, where `n` is the guest's own flip index, the count
`sceVideoOutGetFlipStatus` reports. A flip counts as matched when every pixel's colour channels
are within the tolerance; alpha is not compared (D796). The run keeps its frames under
`compare-<title>/kept` in the traces directory rather than beside every other run's, and writes
a difference picture for each flip that did not match. It fails when any reference flip was not
drawn, differs in extent or does not match, so it can stand as a gate.

`--record <source>` writes the result to the record's `[frame_match]` section: the flips
compared, the flips matched, the tolerance, the largest difference, where the console's frames
came from, and the build and date of the comparison. It is a section of its own rather than part
of `[status]`, because the run compared is not necessarily the run `[status]` keeps, and the latest
comparison replaces the last whatever it found. The table and the title's page then say how many
flips match the console beside the rung. Without `--record` nothing is written, so a comparison
against anything but the console's frames leaves the record alone.

```bash
orbistoun-cli compat reproduce <title> --preview
```

`--preview` is a permissive preview: the title runs as its record says, with every unimplemented
function answering 0 (`ORBISTOUN_DEFAULT_RETURN=ok`). The result goes to the record's `[preview]`
slot, never `[status]` or `[experiment]` - runs under different policies never compare (D312) -
and is printed beside `[status]`. The distance between the two is what the refusals stand in, and
`unanswered` in the preview counts the functions it answered 0. No sheet is made, since the sheet
is the honest run's.

```bash
orbistoun-cli compat reproduce <title> --preview --rank
```

`--rank` then asks the guest which of those answers it needed. The preview is rerun once per
function it answered without an implementation, with that one function refused again
(`ORBISTOUN_WITHHOLD=<name>`). Every rerun stops at the preview's own call count, the
deterministic limit, rather than on the clock. Before them the preview runs twice more under
that budget, and how many imports the three runs differ by is the margin a rerun must clear - a
title whose threads interleave differently from run to run still differs by a few. A rerun that
got less far than all three - a lower rung, or fewer imports - depended on the answer. One within
the margin did not. One that got further than all three had the answer in its way. A rerun that
moved either way is run twice more, and a function whose three runs disagree is listed as
unsettled rather than ranked. Only the
rung and the imports are compared, since calls and frames move with a run's timing. The result
goes to the record's `[preview_dependence]` section, with the budget and the margin, the
furthest set back first: the
refusals standing between the title and the preview's reach, in the order to implement them. Each
rerun is an intervention and is never recorded as a result. The command makes one run per
function, each as long as the record's limit.

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

### Call specs

Most questions for the hardware have one shape: find a function, give it buffers filled with a
byte nothing writes by chance, call it with each set of arguments, and dump the buffers. A call
spec says that as data, so asking needs a file rather than new probe code:

```bash
orbistoun-cli questions --spec <function> > spec.toml     # a draft for one function
orbistoun-cli session <host> --key <key> --out run.obs --script spec.toml
orbistoun-cli probe run.obs --device <name>
```

A spec is a list of steps, each doing one thing. `as` names a step's result for later steps:

| Step | Does | `as` binds |
|---|---|---|
| `resolve = { library = "...", symbol = "..." }` | looks the function up | its address |
| `scratch = { bytes = 0x40, fill = 0xcc }` | takes a probe-owned block filled with `fill` | its address |
| `call = <value>`, `args = [<value>, ...]` | calls with up to six integer arguments | the return value |
| `read = <value>`, `bytes = <n>` | dumps memory | - |

A value is a number, a `0x` string, or a name bound earlier. `questions --spec` takes the
library, NID and arity from the declaration, and gives a block to each argument a run recorded
as a pointer, or failing that, one the knowledge entry calls `ptr`. It reads every block back,
then repeats the call with every pointer null. Setting the integer arms is the asker's job. The
session writes the same transcript `probe` reads, and a spec that stops partway keeps what it
sent. `probe --as-knowledge` turns each call to a resolved function into an edge case of its
entry: the arguments, what came back, and what each block held when next read. That entry is
`measured` only when the operator asserts the target hardware. `scratch` needs a probe that
announces it (obSCEne `REQ-20261012T1848Z-3feb`).

## What no measurement covers

```bash
orbistoun-cli census                                  # both tables
orbistoun-cli census --imports --titles PPSA02664,PPSA21564 --json
orbistoun-cli census --refusals --top 20
orbistoun-cli resolutions <worklog> --reports <obSCEne repository> --since REQ-20261014
```

`resolutions` checks what came back: every resolved request orbistoun filed, its outcome's
status codes and functions looked for among the `OBS|` rows of the logs it cites. A code no row
holds, or a function whose rows say it never bound, is printed, and that request is filed again
rather than ingested. A function no row names is a note: the rows may be keyed otherwise (D840).

`census --imports` lists every import of every module a title in the library runs - its
executable and the modules it ships - whose knowledge entry is missing or not
`measured`/`published`: library, name or hash, its `known_by`, whether orbistoun implements
it, the calls the persisted traces recorded, and the titles that import it. Imports the
title's own modules answer are left out; loading those is the work, not measuring them.
`census --refusals` lists what the persisted traces recorded refusing, by layer: the
translator's shader failures (a trace keeps its first submission's) and the graphics backend's
refused commands over the whole run, each keyed by reason with stage and address stripped. Both
tables rank by titles reached, then by volume.

The census is what a capability request is filed from: group its rows by library or GPU
layer and file one obSCEne request per group, with any reference emulator's answers attached
as predictions (D818, [THE_LOOP.md](THE_LOOP.md#the-capability-sub-loop)).

## A surface layout from addrlib

```bash
tools/addrlib/build.sh                        # builds against oops-mesa's addrlib, with clang 21
target/addrlib/equation 64KB_R_X 8            # a swizzle mode at a texel size, in bytes
```

A layout `tiling.rs` does not model yet is read off Mesa's addrlib under the console's
configuration (family NV, revision `0x82`, `GB_ADDR_CONFIG` `0x4`). It prints each coordinate
bit's term in the block's byte offset, and the terms bits above the block add. It then checks
every texel of two surfaces against row-major blocks under those terms, exiting non-zero on any
disagreement. The terms go into `tiling.rs` as data; the tool is how they were read.

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
implementation is the suspect, scalars included (D179). A named import's dumped calls are dumped
twice: as the guest made the call, and as it returned, marked `(on return, rax ...)`, so an
out-parameter shows what the implementation wrote beside what it answered. Only the stack is
writable by a forced write: the image's runs are protected after relocation.

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
| Call traces, frames, submission dumps | `<data>/traces/runs/<title>/<run>/` | every run, on every outcome |
| Run reports | `<data>/reports/` | `orbistoun-cli report` |
| Learned answers | `<data>/learned.toml` | `orbistoun-cli turn --apply` |
| Names worked out | `symbols/generated.json` | `./bin/orbistoun names`, accumulating |
| Hashes still unnamed | `symbols/wanted.txt` | the same |
| What a title reached | `compat/<title>/report.toml` | `orbistoun-cli compat record` |
| What this machine can contribute | `submission/` | `orbistoun-cli submit export` |
| What is known, and not | `crates/orbistoun-hle/data/knowledge/` | `orbistoun-cli learn` |
| Window captures | `<data>/screenshots/*.png` | the GUI toolbar's capture button (D162) |

`<data>` is the platform data directory, or the binary's own directory in portable mode;
`orbistoun-cli paths` prints it. Each run writes its `trace.json`, frames and dumps into a
directory of its own, so concurrent runs never overwrite each other's, and a title keeps its last
four runs (D865). The previous run is the title's latest that wrote a trace, and `worklist` totals
across each title's latest.

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
