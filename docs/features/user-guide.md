# User guide

Orbistoun runs Prospero-generation guest executables on an ordinary x86-64 machine. Guest
instructions run natively; orbistoun reimplements the operating system and libraries beneath
them and translates the guest's GPU command streams to Vulkan. This page covers what is common
to every feature: the two programs, the window's layout, troubleshooting and sending results.
The other pages cover one feature each.

## Requirements

- An x86-64 host running Windows or Linux. Guest code executes directly on the host CPU.
- A Vulkan device, for the guest's GPU work.
- Titles of your own. Orbistoun ships none; see [the library](library.md).

## The two programs

| Program | What it is |
|---|---|
| `orbistoun-cli` | the command line. Every operation is here, including ones the window has no control for. |
| `orbistoun-gui` | the desktop window: a library, a run, its result and the settings. |

Neither holds behaviour the other lacks: both call the same crates, and the window's run
verdict is the same text the command line prints. A run executes in a worker process, a
second copy of the same binary, so a guest fault ends the worker and leaves the program that
started it running.

## Command-line quickstart

Point `run` at a title's `eboot.bin`, either one `selfish --format title` built or one taken
from hardware:

```bash
orbistoun-cli run <title>/eboot.bin                   # run it; prints the verdict against the last run
OOPS_LOG=debug orbistoun-cli run <title>/eboot.bin    # also print decisions and resolved configuration
OOPS_LOG=trace orbistoun-cli run <title>/eboot.bin    # also print every call
orbistoun-cli report <title>/eboot.bin                # survey the module and persist a report
orbistoun-cli paths                                   # where reports, traces and settings are
orbistoun-cli env                                     # every environment variable orbistoun reads
orbistoun-cli --help                                  # every command
```

From a source checkout, `./bin/orbistoun run <title-id>` is the development wrapper: it finds
the title in the library, rebuilds, refreshes names if they are stale and runs it in one step.
See [WORKFLOW.md](../WORKFLOW.md).

## Starting the window

```bash
orbistoun-gui                              # opens in the view chosen under preferences - general
orbistoun-gui --list                       # open in the list view this time
orbistoun-gui --shell                      # open in the shell view this time
orbistoun-gui --title <name>               # launch a title straight away; returns to the view when it ends
orbistoun-gui --title <name> --playback <file>   # launch it and play captured pad input into it
```

`--title` matches a title's folder name or its title ID, ignoring case. `--list` and `--shell`
together, or either with `--title`, is refused in the terminal before any window opens.

## Two views of one library

- **List view** - the menu strip, the toolbar, the library down the left and the selected
  title's details on the right. This is the working view: every control and every diagnostic
  is here.
- **Shell view** - the library as a wall of tiles in rows (user, titles, settings, power),
  moved through with the pad or the arrow keys and chosen with the south button. It fills the
  window; its settings row has "developer list view" to return to the list.

Both draw the same scan and the same selection. [The library](library.md) describes them in
detail.

## Menus

| Menu | Item | Does |
|---|---|---|
| file | rescan library | reads the library folder again |
| file | shell | switches to the shell view |
| file | quit | closes the window |
| probe | connect... | opens the probe window (see below) |
| settings | preferences... | opens the preferences window (see [running a title](running.md#preferences)) |
| settings | title overrides... | edits the selected title's override file |
| settings | reload settings file | reads `config.toml` again and rescans; discards unsaved preference edits |
| help | documentation... | opens these pages |

## Toolbar

| Control | Does |
|---|---|
| start | runs the selected title. Double-clicking a row does the same. |
| stop | ends the running title. Available on Windows. |
| configure | edits the selected title's override file |
| refresh | reads the library folder again |
| screenshot | writes the window, as drawn, to a PNG in `screenshots/`; hover "saved" for the path (D162) |
| capture input, playback input | record and replay pad input; see [controllers](controllers.md) |
| limit | seconds a run may take, `0` for no limit; applies to the next run |

A control that does not apply is greyed rather than hidden, and its tooltip says why. The
right end of the toolbar shows the running title, or how many titles the last scan found.

## The shell button

The shell button is the system button on the pad, `Home` on the keyboard by default. It
belongs to the window, not the title: a title never sees it.

- A tap opens the overlay over the running title: resume, library, settings.
- A hold opens the power menu: quit the title, close orbistoun, back.

While the overlay is open, the title receives a pad nobody is holding.

## The probe window

probe - connect... opens a window that talks to a conformance probe (obSCEne) listening on a
network address. Enter the address and the key the probe shows when it starts, and connect.
The probe's own account of itself is listed as its claim, not as evidence. Type a command -
`report`, `read <addr> <len>`, `call <addr> <args>` - or use the quick buttons; each answer is
shown as it came off the wire, and a probe that died, timed out or dropped the connection is
shown as that, never as a value. `orbistoun-cli ask` and `orbistoun-cli session` do the same
from the command line.

## Troubleshooting

### A call orbistoun does not implement

An unimplemented call refuses rather than returning success. The value a guest receives is a
placeholder of the form `0xF7FFxxxx`, a range no real firmware code occupies (D670), so a
guest message quoting such a value points at a refusal rather than a real error.
`ORBISTOUN_TAG_PLACEHOLDERS=1` gives each unimplemented function its own placeholder, so the
value names the function. The run's ranked findings say what to do next, and
[the loop](../THE_LOOP.md) describes how a refusal becomes an implementation.

### A fault away from the last call

Read the fault as orbistoun's: a wall belongs to the emulator until hardware shows the same
path failing the same way there (D708). The run result's "last calls before the fault" and
[inspecting a run](inspector.md) are where to start.

### "no titles here"

The scan found no directory holding an `eboot.bin`. The folder it scanned is shown under the
message; set a different one under preferences - general.

### "settings not loaded"

`config.toml` or `shell.toml` did not parse. The window shows the defaults and overwrites
nothing; the message names the file and the error.

### "waiting for its first frame"

The title is running and has not presented a frame. The run result appears when it ends.

### A run that ends on its own

The run limit or the call budget ended it; see [running a title](running.md#limits).

### A missing artefact

Run `orbistoun-cli paths`; it prints every location for this machine and this mode. See
[where it writes](paths.md).

## Sending results

Every run writes its call trace to `traces/`, on the fault path and the time-limit path
alike, with no flag needed. To send what a machine found:

```bash
orbistoun-cli submit export --out submission     # gather this machine's measurements and title records
orbistoun-cli submit check <received-directory>  # re-derive a received bundle against this machine
```

Attach the trace or the bundle to an issue at
[github.com/project-oops/Orbistoun](https://github.com/project-oops/Orbistoun). A received
bundle is checked by measuring again: a claim this machine never measured is reported as
unmeasured, not as a contradiction.

## Data and privacy

Orbistoun is local-first. It sends no telemetry and needs no account. It opens a network
connection only when a command asks for one: a probe connection, `serve` (loopback by
default), `corpus sync`, or a hosted model provider you configure. Settings, saves, reports
and screenshots are plain files under the data root; see [where it writes](paths.md).
