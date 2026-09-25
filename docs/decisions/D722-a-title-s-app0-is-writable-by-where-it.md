# D722 - a title's /app0 is writable by where it is staged, not what it ships

**Status:** decided
**Date:** 2026-09-25

## The question

Neverball never started play. At the level intro it went to `st_nodemo` ("A replay file could not be
opened for writing"): its user data lives under `/app0` (`.neverball`, `Replays/Last.nbr`, `Scores`,
`neverball.log`), and orbistoun's `/app0` is read-only for every title (D250).

On the console it works. The port's own startup shim (oops-apps `neverball/shim/nb_start.c`) states
it: `/app0` is writable, `config_paths` creates `/app0/.neverball`, `config_save` fills it. So some
titles' `/app0` is writable on hardware and orbistoun refused it. Which, and how is it decided?

## The choice

**By storage origin, as the hardware does. Never by anything the title ships.** The operator ruled
out package metadata outright: no `param.json` field, no content-ID publisher prefix. On the console
permissions come from the mount and the process's confinement, not from the package.

- **A retail library image keeps a read-only `/app0`** (D250). A title installed from a package
  mounts its PFS image there, and a write to it is refused.
- **A title staged on the writable user partition gets a writable `/app0`.** Homebrew is staged
  under `/data/homebrew/<id>` (`pros restore`), and launched from there. `/data` is read-write, and
  the title's `/app0` is that directory, so the title can write into it.

**orbistoun models the staging tree.**

- `%APPDATA%\OOPS\titles\data\homebrew\<id>` is inside the one titles library. `corpus sync` unpacks
  a source there when its entry says `target = "staged"`, as `pros restore` stages one on the
  console.
- A run whose module lies under that root is a staged title. So is a loose build run with
  `--staged` (`Request::Run::staged`): a developer iterating on `build/title/<ID>` need not sync
  first.

**A staged title's namespace:**

- **`/data/homebrew/<id>`** is mounted: the title's files, under a writable layer.
- **`/app0` is the same directory**, as on the console. Its top layer is that same writable folder,
  `<overlay>/data/homebrew/<id>`, and `/app0` joins the writable prefixes. A file written through
  either path is visible through the other.
- **The library's files are never written.** Writes land in the per-title overlay, which D422's
  retention policy governs like every other.

**Writes never reach a lower layer.** This holds for every writable mount, not only a staged one:

- A write to a file that exists only below the top layer **copies it up** first
  (`mount::resolve_for_write`).
- A truncating create goes straight to the top layer (`mount::resolve_for_create`).

Before this, a write resolved to the first layer that held the file. For a staged title that is the
library, and for `/data` it is the base tree.

**Removing or renaming a name a lower layer also holds is refused** (`mount::resolve_for_removal`).
Deleting only the top copy would leave the lower one visible, and a correct answer needs a whiteout,
which this overlay does not keep. `mkdir` on such a name is refused the same way, which is its
`EEXIST`.

**Process confinement** is the other half on hardware: an unconfined process (D413's escape) is not
held to its sandbox. orbistoun does not track confinement as state today. When it does, an
unconfined process gets the same writable `/app0`. Neverball does not escape, so storage origin is
what decides it here.

## What it rests on

- D250 and D251: the manifest and the read-only `/app0`.
- D422: per-title overlays and retention.
- The port's hardware-observed use of `/app0` (`nb_start.c`).
- The staging path, `/data/homebrew/<id>`, where the operator's `pros restore` stages titles.
