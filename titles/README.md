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
