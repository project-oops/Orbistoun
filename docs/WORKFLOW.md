# Workflow

What to run, in what order, and how often. This is the cycle the whole project is shaped
around:

> **Execute it. Read what it wanted. Implement the frequent ones. Execute it again.**

Everything else - the container parser, the loader, the stub table, the name search -
exists to make one turn of that loop cheap.

**[THE_LOOP.md](THE_LOOP.md) explains what a turn actually does**, step by step, with a
diagram and an honest account of which steps still need a person. This page is the command
reference; read that one first if the shape is not already familiar.

## Debugging one title

```bash
./bin/orbistoun run PPSA04263
```

**The single command.** It resolves the title id to a module, rebuilds the binary,
refreshes names **if and only if they are stale**, runs the guest under a time limit, and
prints what it asked for. Nothing static a run depends on is left for you to remember.

With no argument it lists what is available locally. A full path works too.

Names are rebuilt when the grammar or the word list is newer than the database, or when
the module is. Searching 2.6 billion candidates takes about ninety seconds - **once for
the whole corpus, not once per module**, because the cost of a sweep does not depend on how
many hashes it is looking for (D213). Paying even that on every debug run of an unchanged
tree would make the fast path slow enough that people start skipping it, at which point
they are debugging against stale names, which is worse than either.

## Every title at once

```bash
./bin/orbistoun sweep
```

The same thing across every module available locally, ending in a ranked list of what
all of them called.

The last section of its output is the answer to "what should I do next":

```
         CALLS  SHARE  MODULES  IMPORT
      87621502  99.9%        1  libkernel::sceKernelDirectMemoryQuery
          1433   0.0%        4  libc::__cxa_atexit
           474   0.0%        4  libc::memset
```

**Ranked by calls, not by count of modules.** A function called four hundred million
times is not four hundred million times more important than one called twice - it is a
guest stuck in a loop waiting for an answer, which is a wall rather than a feature. Both
are worth knowing and the ordering makes the difference obvious.

## Did that help?

Every run ends with the answer:

```
progress
  FURTHER  image+0x13514 -> image+0x1a4c20
  imports  31 distinct (+2), 402 calls (+65)
```

**`FURTHER` means the guest executed code it could not reach before.** That is the
measure this project optimises, and the only one that says a change was worth making.
`BACK` means something regressed - worth knowing immediately rather than three changes
later. `same` on an unchanged tree is the expected result, and the fact that it *is*
reliably the same is what makes any movement attributable.

## What a turn tells you, and what to do about it

| What you see | What it means | What to do |
|---|---|---|
| A name at the top with an enormous count | A guest is spinning on it | **Implement it.** This is the wall |
| `library::0x…` at the top | Same, but not yet named | Extend `crates/orbistoun-names/data/vendor.toml`, re-run |
| A fault instead of a time-out | The guest got somewhere and died | Read the `faulted` block - operation, address, registers, and the calls just before |
| Lots of calls spread thinly | The guest is making progress | Implement the frequent ones and go again |
| `standing` fell while `calls` rose | A stub started answering, and lying | Look at what the new answers unlocked before believing the progress |

Every finding ends in an arrow saying what to do about it. If one does not, the finding is
underspecified and that is a bug in `orbistoun-report::diagnose`, not something for the
reader to work around.

## When the report is not enough

Two escape hatches, both deterministic, both preferable to writing a throwaway script:

```bash
ORBISTOUN_DUMP=memalign,malloc ./bin/orbistoun run PPSA02664
```

**Forces argument dumps for functions that are already implemented.** Dumps are attached to
unimplemented calls by default, on the reasoning that an implementation needs no
explanation - which is exactly backwards when the implementation is the suspect. Scalars
are recorded as well as pointees, because an argument that *is* a value points at nothing
and would otherwise be invisible (D198).

```bash
orbistoun-cli knows sceKernelCreateSema
orbistoun-cli questions --top 20
```

**What is recorded about a function, and what is still guessed at.** Before changing an
implementation, this says what the current behaviour rests on - `published`, `measured`,
`guest-observed` or `assumed` - and what nobody has established. Changing something that
rests on a citation is a different act from changing something that rests on a guess.

## Writing down what a turn produced

Nothing here happens automatically, on purpose. A finding recorded by hand is a finding
somebody decided was true.

```bash
orbistoun-cli learn <function> ...   # a behaviour, with what it rests on
orbistoun-cli compat record <title>  # how far this title got, read off the trace
```

`learn` refuses an entry that does not say where it came from. `compat record` is
transcription rather than opinion - it reads the last trace and writes the numbers.

It writes into one of two slots and picks for you. A run measuring the emulator as it stands
updates `[status]`; a run helped along - a loosened default, or functions answered by name
from `learned.toml` - updates `[experiment]`. Each is compared only against its own slot, so
neither can overwrite the other and **no run is refused for its policy** (D312). `--force`
remains for one case: replacing a better entry inside a slot.

## Cadence

There is no schedule. Each of these is triggered by something, not by a clock.

| Run | When |
|---|---|
| `./bin/orbistoun run <title>` | Constantly. This is the debug loop |
| `./bin/orbistoun sweep` | After any change that could move a guest further |
| `./bin/orbistoun check` | Before considering any unit of work done. Non-negotiable |
| `./bin/orbistoun names` | Included in `sweep`; separately after extending the vocabulary |
| `orbistoun-cli worklist` | Any time. It reads persisted traces, it does not re-run anything |
| `orbistoun-cli questions` | Before implementing anything - it says what is not known about it |
| `orbistoun-cli learn` | Whenever a turn established something. Not automatic, deliberately |
| `orbistoun-cli compat record` | After a run that moved. The run prompts for it |
| `orbistoun-cli harvest <freebsd-src>` | Rarely. When you want a bigger standard-library list |
| `./bin/orbistoun symbols-audit` | Included in `check` and in CI. Never by hand |

## The pieces, if you want to drive them individually

```bash
# What does this module need, without running it?
orbistoun-cli imports titles/SOME-TITLE/eboot.bin

# Work out names for hashes nothing can name yet. A directory is ONE search over the
# whole corpus, not one per module - and it is the only form that can find a name lying
# in one title's strings that explains a different title's import (D213).
orbistoun-cli names titles \
  --out symbols/generated.json --wanted symbols/wanted.txt

# Add what a previous run read out of guest memory. Needs a run to have happened.
orbistoun-cli names titles --from-trace --out symbols/generated.json

# Re-read the modules behind every static record, and confirm each contains its string.
# The tier of claim CI cannot check, checked by whoever has the corpus.
orbistoun-cli audit symbols/generated.json --verify-harvest

# Run one guest and watch what it asks for.
orbistoun-cli --symbols-db symbols/generated.json \
  run titles/SOME-TITLE/eboot.bin --limit 20

# Aggregate every run so far into one work list.
orbistoun-cli worklist --top 40
```

## Where the input comes from

Two mechanisms, and the rule for which is which is worth stating because nothing else here
does.

| | Where | What belongs in it |
|---|---|---|
| **Settings** | `<data>/config.toml` | How the emulator behaves: entry presentation, thread placement, memory behaviour, the library folder, what unimplemented functions answer. Persistent, and the thing you edit to bisect a stub (D166) |
| **The environment** | Variables, listed by `orbistoun-cli env` | The two settings that *cannot* live in the file - because they decide where the file is - plus every **diagnostic**, which is meant to go away rather than persist |

`orbistoun-cli paths` says where `config.toml` is; `orbistoun-cli env` lists the variables.

The split is structural rather than stylistic. `config.toml` is composed of settings owned
by the loader, the kernel and the HLE layer, so it lives near the top of the dependency
spine. The environment registry lives at the bottom, because `orbistoun-paths` needs it to
work out where the data root - and therefore `config.toml` - is (D221).

## The diagnostics

```bash
orbistoun-cli env
```

**That is the list, and this document deliberately does not repeat it.** It prints every
variable orbistoun reads, what each is for, an example you can copy, which crate reads it,
and what is set right now - from the one registry in `orbistoun-env`, so a diagnostic added
anywhere shows up without anybody remembering to write it down here. A table copied into
this file is a second list, and second lists drift (D221).

What is worth saying here is the shape rather than the contents:

- **They are not settings.** Each answers one question, once. A diagnostic left configured
  for three weeks stops being an experiment and becomes an undocumented workaround for a
  bug nobody found (D185), which is why they live in the environment and why a future
  `.env` file would be allowed to carry settings and refused for these.
- **Every one is recorded in the run's conditions**, so a verdict taken under a diagnostic
  is never compared with an ordinary run as though they measured the same thing (D181).
- **An import is matched by name or by any part of its label**, so an unnamed function is
  addressed by its hash: `ORBISTOUN_DUMP=0x6abac2f3dc6f8cee`. That is the point rather than
  a convenience - the functions most worth experimenting on are the ones nothing has named.
- **A request that matches nothing says so**, rather than reporting a run that changed
  nothing. And a variable that is *nearly* one of these - `ORBISTOUN_STACK_FIL` - is
  reported as unrecognised, because a misspelled variable is an absence rather than an
  error and would otherwise produce an ordinary result that gets believed.

```bash
# What is the wall being handed, and does planting a base move the fault?
ORBISTOUN_DUMP=0x6abac2f3dc6f8cee ./target/release/orbistoun-cli run titles/X/eboot.bin
ORBISTOUN_WRITE=0x6abac2f3dc6f8cee:0:0x11000000 ./target/release/orbistoun-cli run titles/X/eboot.bin
```

Only the **stack** is writable by a forced write. The image's runs are protected after
relocation, so planting into one would fault inside the emulator and produce a crash with no
relation to the guest.

### Two of them compose: which slot, then who touched it

`ORBISTOUN_WATCH` and `ORBISTOUN_WATCHPOINT` sound like the same thing and are not. The
first copies a region before the run and diffs it afterwards, so it says **which words ended
up different** - and therefore which nobody wrote. The second arms an x86 debug register, so
it says **which instruction touched an address**, how often, and what it saw there.

Run them in that order and the second needs no guesswork about where to point:

```bash
# 1. Which words in this structure did nobody ever write?
ORBISTOUN_WATCH=0x4000019e9c00+0x80 ./target/release/orbistoun-cli run titles/X/eboot.bin
# 2. Who reads the one that stayed zero?
ORBISTOUN_WATCHPOINT=0x4000019e9cb0:rw ./target/release/orbistoun-cli run titles/X/eboot.bin
```

Neither step reads the guest's code, which is what keeps the pair inside principle 1 and
makes it a candidate for automation rather than a manual detour (D276).

Two things the hardware imposes, both refused out loud rather than rounded off: there are
**four** watchpoints, of one, two, four or eight bytes, each aligned to its own length; and
x86 has no read-only encoding, so `rw` traps writes as well.

One thing it imposes that cannot be refused: a data breakpoint fires *after* the access
completes, so the instruction pointer reported belongs to the **next** instruction. Every
line says `after the access at` for that reason. Naming the instruction that actually did it
would mean decoding it, which is disassembly of a vendor binary (D277).

## Where the output goes

Nothing important is left on a terminal. A run that takes ten minutes and prints its
findings to a scrollback has produced nothing durable.

| Artifact | Where | Written by |
|---|---|---|
| Call traces | `<data>/traces/*.json` | Every run, on both the fault and time-limit paths |
| Run reports | `<data>/reports/` | `orbistoun-cli report` |
| Names worked out | `symbols/generated.json` | `./bin/orbistoun names`, accumulating |
| Hashes still unnamed | `symbols/wanted.txt` | Same, accumulating |
| What a title reached | `compat/<title>.toml` | `orbistoun-cli compat record` |
| What this machine can contribute | `submission/` | `orbistoun-cli submit export` |
| What is known, and not | `crates/orbistoun-hle/data/knowledge/` | `orbistoun-cli learn` |
| Window captures | `<data>/screenshots/*.png` | The GUI toolbar's **capture** button (D215) |

`<data>` is the platform data directory, or the binary's own directory in portable mode.
`orbistoun-cli paths` prints it.

Traces are keyed by module, so a sweep leaves one per title rather than each overwriting
the last, and `worklist` totals across all of them.

## Why the guest is given a time limit

A guest whose imports all return "unimplemented" does not necessarily crash. One
commercial executable here ran for **ten minutes** without faulting, calling a single
function four hundred million times - which looks like success and is really a wait that
will never end.

Killing it from outside loses the trace, which is the only thing the run was for. So the
worker stops it itself, writes what it learned, and exits with a status meaning exactly
that (D066). `--limit 0` removes the limit when you genuinely want it.

## Starting from nothing

```bash
./bin/orbistoun doctor --fix   # what is missing, and install it
./bin/orbistoun check          # confirm the tree is sound
./bin/orbistoun run <title>    # first turn
```

`doctor` runs automatically before anything that needs a toolchain, so a missing
requirement surfaces as one clear line rather than as a build error three steps in.
`--fix` installs the optional tools and enables the pre-push hook; it will not install a
Rust toolchain, because that is a machine-wide decision that belongs to you.

With no modules under `titles/`, `sweep` still works and says so: the name generator
produces candidates, but confirming one needs a real import table to collide against.
See [PROVENANCE.md](PROVENANCE.md) for exactly what that does and does not imply.

## Asking a model for vocabulary

Naming is limited by the grammar, so the way to name more imports is to have more words.
`orbistoun-propose` asks a local model for them, one grammar position at a time, and lets
the hash decide - a wrong suggestion costs a sweep and vanishes.

```bash
ROUNDS=3 cargo test -p orbistoun-propose --release --test live -- --ignored --nocapture
```

Opt-in, because it downloads and runs a model on the GPU. `ROUNDS` defaults to 3 and there
is little reason to raise it: a measured 36-round run earned three names, and effectively
all of the yield was in the first round of each position.

Words the hash confirms land in `symbols/proposed-*.txt`. Promoting one into
`crates/orbistoun-names/data/vendor.toml` is deliberate and manual - put a noun in `object`
and a suffix in `tail`, then re-run the name search:

```bash
./bin/orbistoun names
```

## Turning the loop without reading the findings

`orbistoun-turn` reads the run's ranked findings and performs the ones that are
mechanical - sweeping every argument of the call that led into a fault, asking every other
diagnostic axis about the faulting address - then stops at the ones that need a person.

```bash
cargo test -p orbistoun-propose --release --test turn -- --ignored --nocapture
```

It is a dispatcher, not a chooser. A boot costs about 0.13 seconds, so every sweep it runs
is exhaustive rather than ranked; nothing here decides what is *worth* trying (D231).
