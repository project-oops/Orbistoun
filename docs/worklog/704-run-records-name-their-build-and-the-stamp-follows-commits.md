# 704. Run records name the build that made them, and the stamp follows commits

**2026-09-19** — inbox `-8ae5`: every `[status]`/`[experiment]` block in `compat/` read
`build = "0.1.0"`, and the local stamp was frozen at `3b95d21-dirty` while `HEAD` had moved to
`b4f7e73`. So no record could be tied to the tree that produced it, and a regression like `-2180`
could not be bisected from the records. Two causes, one in each repo, both fixed.

## The record named the crate version, not the build

`orbistoun-worker` recorded `env!("CARGO_PKG_VERSION")` - `0.1.0`, the same on every run - as a run's
`build`. It now records `orbistoun_env::build::line()`, the commit the worker was built from with a
`-dirty` suffix for an uncommitted tree, which is the stamp `orbistoun-cli paths` already prints. A run
recorded now carries `b4f7e73-dirty`, a tree somebody can check out, instead of a version that names
nothing.

## The stamp did not follow commits on a branch

`oops-build`'s `watch_git` registered `cargo:rerun-if-changed` on `.git/HEAD` alone. On a branch,
`HEAD` holds `ref: refs/heads/<branch>` and **does not change when a commit lands** - only the ref it
names does - so the build script never re-ran, and the stamp named whatever commit happened to be out
when it last ran for some other reason. That is why it was stuck at `3b95d21` seventeen commits back;
its own doc even claimed it was "refreshed when the commit moves," which was false for the ordinary
branch case.

`watch_git` now also follows the ref `HEAD` points at (`.git/refs/heads/<branch>`) and the reflog
(`.git/logs/HEAD`, the catch-all that appends on every move of `HEAD` and covers a packed ref), so a
commit on the checked-out branch re-runs the script and refreshes the stamp. The parse of `HEAD` is
split into a pure `parse_head_reference` and unit-tested: a symbolic `HEAD` names its ref, a detached
one (a bare commit id, whose own file already changes on a commit) names nothing.

## Verified

- `orbistoun-cli paths` now prints `v0.1.0 - b4f7e73-dirty`, the current `HEAD`, where it read
  `3b95d21-dirty` before - the stamp refreshed on rebuild once the build script started following the
  ref.
- The build script's output now carries `rerun-if-changed` for `.git/HEAD`, `.git/logs/HEAD` and
  `.git/refs/heads/main`, so a commit (which writes both the ref and the reflog) re-runs it.
- `parse_head_reference` tested (branch → ref, detached/empty → none).

The one step the loop cannot run is the literal acceptance (2): make a commit, rebuild, confirm `paths`
names the new commit. The no-commit policy holds, but the mechanism is in place and demonstrated - the
watched paths are exactly the ones a commit writes, and the stamp is shown tracking `HEAD` - so a
commit will refresh it. Likewise a title re-run will write `b4f7e73-dirty` into its record; the
records already on disk keep their old stamp until a run rewrites them.

## Gate state

Both repos green: `./bin/orbistoun check` and `./bin/oops-libs check` pass end-to-end; `oops-build`
tests (8, one new) and clippy/fmt clean; identity scan clean in both. Corpus unchanged. No commit -
the fix spans `orbistoun-worker` and `oops-libs/crates/oops-build`, both left uncommitted for the
operator.
