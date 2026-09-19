# 709. Three unused dependencies removed, the machete lint job green

**2026-09-19** — inbox `-caf9`, slice 3 (the cargo-machete slice): the CI `lint` job
(`Audit + deny + machete`) has been red in part because `cargo machete` found three crates
declaring a dependency they do not use. CI builds are strict, so an unused dependency is a hard
failure there. This removes the three.

## What was flagged, and that each is genuinely unused

`cargo machete` names a dependency used nowhere in a crate's source. It can be wrong - a crate
reached only through a macro, a re-export, or a `#[cfg]`-gated path can read as unused - so each
was checked by hand before removing, not taken on the tool's word:

- **`orbistoun-cli` → `tracing-subscriber`.** The only mention left in the crate is a comment in
  `main.rs`: "Was five lines of `tracing_subscriber` here … the shared version" - the subscriber
  setup moved to the shared logging layer, and the dependency was left behind. `tracing` itself
  stays (its macros are still used); only the subscriber crate is gone.
- **`orbistoun-firmware` → `orbistoun-core`.** No `orbistoun_core` path anywhere in the crate.
- **`orbistoun-gui` → `orbistoun-nid`.** No `orbistoun_nid` path anywhere in the crate.

Each was checked across the whole crate directory, not just `src/` - tests, benches and examples
included - and each has zero references. All three are ordinary `{ workspace = true }` lines,
removed with no other change.

## Verification

- `cargo machete` now reports "didn't find any unused dependencies in this directory."
- `./bin/orbistoun security` - the exact `Audit + deny + machete` the CI job runs - passes
  end-to-end (exit 0): cargo-audit clean, cargo-deny `advisories ok, bans ok, licenses ok,
  sources ok`, machete clean.
- `./bin/orbistoun check` passes: the three crates and everything downstream of them still
  compile, clippy `-D warnings` clean, tests pass. Removing a dependency can only break a build
  that was leaning on it through feature unification, and nothing was - the full workspace is
  green.

## Scope

This clears **slice 3** of caf9. It does not touch the aarch64 ABI (slice 2, worklog 708) or the
symbol-provenance audit (slice 4), which is the one caf9 item still open. No commit.
