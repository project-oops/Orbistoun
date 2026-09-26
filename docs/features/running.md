# Running a title

A run loads a title's container, maps its segments, resolves its import table and starts the
guest in a worker process. The guest's instructions run natively; everything orbistoun does
is the operating system beneath them. What a run produces is a record of what the guest asked
for and what it got, compared with the previous run of the same title.

## Starting and stopping

In the window, select a title and press start, or double-click it. While it runs, the right
side of the window shows its picture (see [graphics](graphics.md)) and the toolbar shows its
name. Stop, available on Windows, ends the worker. "quit the title" in the shell button's
power menu first tells the title it is being closed, then ends the worker.

On the command line:

```bash
orbistoun-cli run <title>/eboot.bin      # execute; prints the verdict against the last run
orbistoun-cli report <title>/eboot.bin   # survey the module, persist a report, print the delta
orbistoun-cli verify <title>/eboot.bin   # how much of the import list the symbol database names
```

| `run` option | Does |
|---|---|
| `--limit <seconds>` | seconds of guest execution before the run is stopped and reported; default 20, `0` for none |
| `--calls <n>` | imports the guest may call before it is stopped; default 20,000,000, `0` for none |
| `--profile <name>` | present a named machine profile instead of `shell.toml`'s; see [machine profile](#machine-profile) |
| `--input <file>` | play a pad script on player 1; see [controllers](controllers.md) |
| `--staged` | run a loose build as a staged title, with a writable `/app0` |

## Limits

A run ends at the first of: the guest exits, the guest faults, the time limit passes, or the
call budget is spent. The call budget is the deterministic limit: two runs of one build stop
at the same call, so a verdict between them measures the change rather than the machine. The
time limit is the backstop for a guest that stops calling imports.

The window's runs take the limit from the toolbar (or preferences - general) and the budget
from `run-call-budget` under `[library]` in `config.toml`. Both default to `0`, no limit,
because a title launched from the window is being played.

## The run result

When a run ends, the detail panel shows what it produced, top to bottom:

| Part | Shows |
|---|---|
| progress | the verdict and its one-line summary; `imports +n, calls +n` against the previous run; the fault, and the previous run's fault beside it |
| stack line | how many calls the guest made, and whether any arrived on a misaligned stack |
| last calls before the fault | the final calls, each with its first argument, when the run faulted |
| what it asked for | every import the guest called, with its call count |
| events | anything else the run reported |

A run that could not start shows the reason in red instead.

## Verdicts

`run` compares a run with the previous run of the same module and prints one verdict; the
window shows the same words.

| Verdict | Means |
|---|---|
| `FURTHER` | the guest executed code it could not reach before |
| `same` | nothing moved |
| `BACK` | the guest reached less of the interface than it did |
| `MIXED` | more of the interface along a different path, or further along its path with less of the interface |
| (none) | the first run of this module; nothing to compare against |

`FURTHER` is the measure of progress. A verdict is differential: it says that something
changed, not that an answer was correct. Correctness needs an oracle, which is a probe
([obSCEne](https://github.com/project-oops/obSCEne)) run on real hardware through
[Prosperous](https://github.com/project-oops/Prosperous).

A change that replaces a plausible answer with a refusal can make a run stop earlier. That is
the intended trade: an unimplemented call refuses at the call, so an early stop names its
cause, where a stub returning success lets the guest carry damage far from where it began.

## Reports

`report` surveys a module, persists a run report under `reports/`, and prints the delta
against the previous run of the same title: the reached state, the unresolved-import count,
and which imports became newly resolved or newly unresolved.

`verify` answers a narrower question, how much of a module's import list the symbol database
can name, printed as `N of M imports named`. It is a naming measure, not a progress one; see
[names and hashes](naming.md).

## Log levels

A run is quiet unless asked:

```bash
OOPS_LOG=debug orbistoun-cli run <title>/eboot.bin     # decisions and resolved configuration
OOPS_LOG=trace orbistoun-cli run <title>/eboot.bin     # per-call detail
OOPS_LOG=warn,orbistoun_loader=debug orbistoun-cli run <title>/eboot.bin
```

`OOPS_LOG` and `RUST_LOG` are both read; `OOPS_LOG` is the one every tool in the collection
answers to. The levels mean the same everywhere: `error` is giving up, `warn` is a surprise
that did not stop the work, `info` is an action with a side effect, `debug` is decisions,
`trace` is per item.

## Preferences

settings - preferences... opens the preferences window. A list of panes is on the left; save
writes `config.toml` and `shell.toml` together, and rescan library reads the folder again.
Settings apply to the next run.

| Pane | Sets |
|---|---|
| general | library folder (and the folder it resolves to), run limit, and the view the window opens in |
| entry | how control reaches the guest's first instruction: the convention (function or process) and what the first argument register holds. The reporting and handoff choices are diagnostics, and a run under one is not compared with an ordinary run. |
| threads | how many cores the guest is told it has and how many are usable, and how guest affinity requests are handled: observe, map or strict |
| memory | whether direct memory is mapped for real; see [memory](memory.md) |
| shell | the machine's users and which one is signed in, the language as a BCP 47 tag (`en-GB`), and which button confirms, south or east |
| controllers | ports, what drives each, and the key for each button; see [controllers](controllers.md) |

A user's name reaches the guest as text. The language and confirm-button settings reach a
guest only through an encoding measured on hardware; a question with no measured encoding is
refused and counted rather than answered with an invented value (D311). User identifiers are
never reused, because save data is keyed on them.

## Machine profile

orbistoun's `Machine` presents a platform along four axes - generation, kind, revision and
firmware - configured in the `[machine]` table of `shell.toml` (D394).

| Key | Values | Default |
|---|---|---|
| `generation` | `prospero`, `orbis` | `prospero` |
| `kind` | `cex` (retail), `dex` (development kit), `tex` (test kit) | `cex` |
| `revision` | `base`, `pro` | `base` |
| `firmware` | the system software version, packed as the guest reads it: 12.40 is `0x1240` | `0`, unset |
| `kernel-release` | what `kern.osrelease` answers | empty, unset |

The generation, kind and revision default to a retail base Prospero-generation machine. The
version fields refuse by default: `firmware = 0` and an empty `kernel-release` refuse the calls
that read them, so an unconfigured emulator claims no particular firmware. `software-version`,
`kernel-version`, `kernel-sdk-version` and `hardware-model` follow the same rule.

The reference configuration is a retail Prospero-generation machine on system software 12.40,
as measured on hardware:

```toml
[machine]
generation = "prospero"
kind = "cex"
revision = "base"
firmware = 4672                    # 0x1240 = 12.40
kernel-release = "0.0-prototype"   # what kern.osrelease returns on that machine
```

`orbistoun-cli run --profile prospero-cex-12.40` presents that machine, with every measured
field, for one run without editing `shell.toml`. The profiles are in
`crates/orbistoun-shell/data/machine-profiles.toml`.
