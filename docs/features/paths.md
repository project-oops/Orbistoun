# Where Orbistoun writes

Everything Orbistoun produces - logs, traces, reports, screenshots, title data, savestates,
settings - hangs off one root. `orbistoun paths` prints them for the machine you are on, in the
mode you are in, which is always more reliable than a page like this one.

## The three ways the root is chosen

In order. The first that applies wins.

1. **Portable** - `./.portable/` beside the executable. Nothing is written anywhere else.
2. **`ORBISTOUN_DATA_DIR`** - an explicit relocation for anyone who wants one.
3. **The collection's directory** - `%APPDATA%\OOPS\` on Windows, `~/.local/share/OOPS`
   on Linux, the equivalent on macOS. **Shared with the sibling projects**, which is the
   point: a save Prosperous pulls off real hardware lands in the tree Orbistoun mounts.

**Portable deliberately outranks the environment variable.** If a variable in your shell could
move data outside the portable root, then "does not touch anything outside its own directory"
would be a suggestion rather than a guarantee - and the whole point of portable mode is that it
is a guarantee.

## Turning portable mode on

Any of these, and they are OR'd:

- a `.portable` directory beside the executable - the durable way, and what the preferences
  toggle creates
- `ORBISTOUN_PORTABLE_MODE=1` - also `true`, `yes`, `on`
- an executable whose own filename contains `portable`, so a downloaded build can announce
  itself with nobody configuring anything

An unrecognised value is **not** on. Reading `ORBISTOUN_PORTABLE_MODE=no` as "yes" would be
exactly the kind of surprise portable mode must never have.

## Two roots, and which is which

Windows distinguishes data that follows you between machines from data that does not, and so do
Linux and macOS. The test for which side something is on: **can you get it back without the
console?**

| | |
|---|---|
| `%APPDATA%\OOPS\` | `titles/`, `saves/`, `overrides/`, `reports/`, `screenshots/`, `orbistoun.toml`, `learned.toml` |
| `%LOCALAPPDATA%\OOPS\` | `models/`, `runtime/`, `shaders/`, `filesystem/`, `traces/`, `logs/` |

Models and runtimes download again, shaders compile again, the base filesystem is materialised
from a manifest, and a trace is one re-run away. A report measured against real hardware is not,
and neither is an override you typed.

**In a portable run they are the same directory**, because the point of portable mode is that
everything is on the stick.

`titles/<title>/` holds that title's guest filesystem and its savestates, so everything one
title accumulated is in one place - and the guest filesystem is the tree Prosperous fills from
real hardware.

## Sending logs to a file

Orbistoun logs to the terminal by default. To keep them:

```bash
OOPS_LOG=debug orbistoun run <title> 2> run.log
```

Levels and filtering are the same in every tool in the collection - see
[running a title](running.md).

## Two things worth knowing

**Nothing is written until something needs writing.** Starting the window does not create a tree
of empty directories.

**The root moved, twice.** It was `%APPDATA%\orbistoun\data\`, then briefly a directory of
this project's own under a shared parent, and it is now the collection's directory with every
sibling. Nothing has shipped, so there was nothing to migrate but one developer's machine.
