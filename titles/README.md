# titles/

Test material. **Nothing in this directory is ever tracked by git** - only this
README, via an explicit exception in `.gitignore`.

Two things live here:

- **Homebrew synced by tooling** - `orbistoun-cli corpus sync` populates it from a
  manifest pinned by commit or release-asset hash, so a clean machine reproduces the
  same corpus (see [docs/DECISIONS.md](../docs/DECISIONS.md) D042).
- **Anything you drop in manually** - your own material, used for development and QA.

In development builds this directory is added to the library paths by default, so it
shows up in the GUI alongside any other configured library. It is not a special case
in the code: just another configured path.

## Titles load from more than one place — this folder is not the only one

**`orbistoun run <id>` resolves a title from the data directory, not from this repo
folder.** The data directory is chosen at runtime by `orbistoun-paths`:

- **portable mode** (a `.portable` sentinel beside the binary) → the data root sits
  beside the binary;
- **`ORBISTOUN_DATA_DIR`** → wherever it points;
- otherwise → the current user's OS application-data location.

Its `titles/<id>/` holds what an installed or `corpus sync`-ed title actually runs:
the `eboot.bin`, the packaged asset archives, any generated caches, and the `fs/`
overlay. This repo's `titles/` is a *separate* library path added only in development
builds — so the same launcher can show both, and a title you ran may have its real
content in the data directory rather than here.

**So do not conclude a title's files are missing by searching this folder.** Run
`bin/orbistoun paths` (or `orbistoun-cli paths`) to print every resolved location —
including the data-directory `titles/` a run reads from — and look there.

## Why it is called `titles/`

Not `games/`, `dumps/`, or `firmware/` - those path names are **banned by the
provenance guard** (`.github/workflows/ci.yml`, `.githooks/pre-push`,
`bin/orbistoun provenance`) and would fail the build if anything under them were ever
tracked. `titles/` is exempted from that guard precisely because its contents are
expected to exist locally and expected never to be committed.

## The rule this exists to protect

Per [CLAUDE.md](../CLAUDE.md) principle 1, no firmware, keys, decrypted content, or
guest binaries belong in this repository. Keeping them here - ignored, exempted, and
documented - is how that stays true while still having something to test against.

Two layers of protection, deliberately:

1. `.gitignore` stops the contents being staged in the first place.
2. The provenance guard fails CI and blocks a push if anything of this shape is
   tracked anywhere - it inspects the index, not the working tree, so it catches a
   `git add -f` that bypassed layer one.

The guard is the control; the ignore rule is a convenience.
