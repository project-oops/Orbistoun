# D221 - Every environment variable is declared in one crate


**decided** · 2026-08-24 · raised by the user, who asked where the central list was

There wasn't one. Nine variables across five crates, and the list of them existed nowhere -
which costs three things, all of which were being paid:

- **A typo does nothing.** A flag spelled wrongly is refused; `ORBISTOUN_STACK_FIL=5a` runs
  an ordinary experiment and reports an ordinary result. Catching that needs a list of what
  is real.
- **Documentation drifts.** The diagnostics were described in three decision entries and
  then hand-copied into a table in `docs/WORKFLOW.md`. That table was a second list.
- **Nothing stopped another appearing.** A crate could read a new variable and nobody would
  find out until somebody grepped.

`orbistoun-env` declares all of them, with a summary, a copyable example, and which crate
reads it. `orbistoun-cli env` prints from the registry, so the WORKFLOW table is now a
pointer at the command rather than a copy of its output.

### The first attempt got it wrong, which is the argument for the crate

The typo check was built *before* the registry, so it needed a hand-written list of names
to excuse - and that list re-typed constants that already existed in `orbistoun-paths`, and
a second copy of the five diagnostic names beside the ones `from_env` used. Two fresh
duplications, introduced in the hour after removing three others.

With a registry there is no exclusion list at all: "is this a real variable" is a lookup.

### Settings and diagnostics are different, and the field says so

A **setting** configures the emulator and is meant to persist. A **diagnostic** changes the
program in order to learn something and is meant to go away. That distinction was already
being made implicitly and badly; it is a field now.

It is what makes the `.env` question answerable. **Settings may come from a file;
diagnostics may not** - a poisoned heap left in a file for three weeks stops being an
experiment and becomes an undocumented workaround for a bug nobody found (D185).

`.env` support is **not built**. It is designed for and nothing needs it: `config.toml`
covers settings, and the two that cannot live there - because they decide where
`config.toml` is - are the ones set once per machine.

### Why it is not called `orbistoun-config`

Raised, and rejected on structure rather than taste. `config.toml` is handled by
`orbistoun_service::FileConfig`, which is **composed of settings owned by
`orbistoun-loader`, `orbistoun-kernel` and `orbistoun-hle`** - so it sits near the top of
the spine and cannot move. This crate sits at the bottom with no dependencies, because
`orbistoun-paths` needs it to resolve the data root, which is where `config.toml` lives.

They are at opposite ends by necessity, so a crate named for configuration that did not own
the configuration file would over-promise - the same failure as labelling a button
*screenshot* when there is no guest frame (D215). `orbistoun-env` is what it is, and stays
accurate if `.env` ever lands, since a dotenv file is the environment by convention.

The underlying complaint was fair though, and was a documentation gap rather than a naming
one: **two configuration mechanisms and nothing saying which is which.** `config.toml`
appeared in no user-facing document at all. `docs/WORKFLOW.md` now states the split, and
`orbistoun-cli env` points at the file for the settings it does not carry.

### What could not be centralised, and why

- `option_env!("ORBISTOUN_COMMIT")` is a macro and needs a literal. Declared in the registry
  anyway, so the listing and the typo check know about it.
- `ORBISTOUN_LIMIT` belongs to `orbistoun.sh` and no Rust reads it. Declared for the same
  reason: a real variable that is undeclared would be reported as a misspelling, which is
  worse than not listing it.
- `orbistoun-llm` is another session's crate and reads its own key. Declared, not migrated.

The registry and the individual constants are the one duplication this cannot remove - Rust
cannot enumerate a module's constants - so it is checked instead, by a test that fails when
the two disagree. Same shape as `Paths::all_dirs`, whose equivalent test caught a missing
entry earlier the same day.

