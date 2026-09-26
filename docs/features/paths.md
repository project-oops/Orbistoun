# Where orbistoun writes

Everything orbistoun reads and writes - settings, titles, saves, reports, screenshots,
traces, logs - hangs off one resolved root, and orbistoun never writes outside it.
`orbistoun-cli paths` prints every location for the machine and mode it runs in.

## Choosing the root

The first rule that applies wins:

1. **Portable** - the `.portable` directory beside the executable is the root, and nothing is
   written anywhere else.
2. **`ORBISTOUN_DATA_DIR`** - an explicit path.
3. **The collection's directory** - `%APPDATA%\OOPS\` on Windows, `~/.local/share/OOPS/` on
   Linux. It is shared with the other OOPS projects, so a save Prosperous copies off real
   hardware lands in the tree a title's filesystem mounts.

Portable outranks the environment variable, so no variable can move data outside a portable
root.

## Portable mode

Any one of these turns it on:

- a `.portable` directory beside the executable. The directory is both the sentinel and the
  root, so a portable install stays portable.
- `ORBISTOUN_PORTABLE_MODE` set to `1`, `true`, `yes` or `on`, in any case. Any other value is
  off.
- an executable whose filename contains `portable`, in any case, such as
  `orbistoun-portable.exe`.

## Data and cache

Material that can be rebuilt goes to a second root, the platform's cache directory:
`%LOCALAPPDATA%\OOPS\` on Windows, `~/.cache/OOPS/` on Linux. The test is whether it can be
recovered without the hardware.

| Root | Holds |
|---|---|
| data | `titles/`, `payloads/`, `packages/`, `console/`, `overrides/`, `reports/`, `screenshots/`, `config.toml`, `shell.toml`, `learned.toml` |
| cache | `traces/`, `logs/`, `filesystem/`, `shaders/`, `models/`, `runtime/` |

A report measured against real hardware and an override somebody typed cannot be regenerated,
so they are data. Traces are one re-run away, shaders recompile, the base filesystem is
rebuilt from its manifest, and models and runtimes download again. In portable mode, and
under `ORBISTOUN_DATA_DIR`, both roots are the same directory.

| Path | Holds |
|---|---|
| `titles/` | the title library: one directory per title (see [the library](library.md)) |
| `titles/<title>/fs/` | the title's writable filesystem, keyed by guest path and merged over the base tree while it runs |
| `titles/<title>/savestates/` | the title's save states |
| `titles/data/homebrew/<id>/` | staged titles, with a writable `/app0` |
| `payloads/` | single executables run directly rather than installed |
| `packages/` | packages that have not been installed |
| `console/` | the writable storage shared by everything run with the system filesystem view |
| `overrides/<title>.toml` | per-title overrides |
| `config.toml` | how the emulator is configured: library, limits, entry, threads, memory, controllers |
| `shell.toml` | what the emulated machine is set to: users, language, confirm button, [machine profile](running.md#machine-profile) |
| `learned.toml` | policy the loop worked out for itself; entries in `config.toml` win, and deleting it undoes all of it |

A title's writable filesystem keeps what the guest wrote between runs.
`ORBISTOUN_SANDBOX=ephemeral` empties it at the start of each run instead.

## Logs to a file

Orbistoun logs to the terminal. To keep a run's log:

```bash
OOPS_LOG=debug orbistoun-cli run <title>/eboot.bin 2> run.log
```

The levels are listed under [running a title](running.md#log-levels).
