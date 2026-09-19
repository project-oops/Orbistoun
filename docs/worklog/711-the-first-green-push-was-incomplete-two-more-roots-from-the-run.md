# 711. The first green push was incomplete - two more roots the run surfaced

**2026-09-19** — inbox `-caf9`: worklog 710 read the pre-fix CI logs and concluded the six red jobs
were three root causes with "no remaining code to write." The first push (`7588806`) proved that
half right and half premature: it turned **Audit+deny+machete** and **Symbol-provenance** green,
but the run on it (`35461857697`'s predecessor `35461010727`) left four jobs red on two causes a
Windows host could not have surfaced. This records them and the fix, and corrects 710.

## What the run showed, and why the local checks missed it

Post-push status: green — Audit+deny+machete, Symbol-provenance, Provenance-guard,
Knowledge-provenance, Test(windows). Red — Check+fmt+clippy, Rustdoc, Test(ubuntu), Test(macos).
Two distinct errors under them:

- **A second Linux dead_code.** `orbistoun-llm`'s `newest_versioned` is called only from the
  `#[cfg(target_os = "windows")]` launcher-discovery block (it looks for `claude.exe`) but was
  defined ungated, so it is dead on Linux. Slice 1 (worklog 707) gated the *worker*'s host-only
  code; this was the same shape one crate over, and it was missed because `orbistoun-llm` is one
  of the five crates a Windows host cannot cross-check (it pulls `ring`, which needs a Linux C
  cross-compiler). **Why Symbol-provenance went green while the others stayed red on it:** that
  job builds the CLI, and the CLI does not depend on `orbistoun-llm`; only the whole-workspace
  jobs (Check+fmt+clippy, Test, Rustdoc) compile it. Gated `newest_versioned`
  `#[cfg(target_os = "windows")]` to match its caller.

- **macOS `error[E0463]: can't find crate for core`.** The slice-2 cross-target step ran
  `rustup target add x86_64-apple-darwin` from the default directory, which added the target to
  *stable* - while the build runs under the `rust-toolchain.toml`-pinned 1.97.1, which then had no
  std for it. Fixed with `working-directory: OOPS/orbistoun` on that step, so rustup honours the
  toolchain file. This is exactly the failure `release.yml` documents and solves the same way; the
  slice-2 change (worklog 708) copied release.yml's *target* choice but not its working-directory,
  which is the load-bearing half.

## The honest correction to 710

710 said "no remaining code to write." That was true only of the causes visible in the *pre-fix*
logs - it could not see a dead_code the failing compile never reached (the workspace stopped at
the worker errors before llm), nor a std-resolution bug in a workflow step that had never run. The
run is the oracle here, not the local reasoning: the whole reason these escaped `./bin/orbistoun
check` is that it runs on one host and CI runs three. Recorded so the lesson is not "710 was
wrong" but "a cross-platform claim is only confirmed by the cross-platform run."

## A third root, from my own fix - and the end of pushing blind

The run on `c15f46b` (`35461857697`) showed the `newest_versioned` gate was right but incomplete:
gating it orphaned `use std::path::{Path, PathBuf}` - `Path` is now referenced only by the
`cfg(windows)` block, so `error: unused import: Path` in `orbistoun-llm`, which failed Test(ubuntu),
Rustdoc **and** Test(macos) (macOS's E0463 was fixed - it now compiles past the toolchain and hits
this same error). Split the import: `PathBuf` stays unconditional, `Path` gated `cfg(windows)`.

**This one was verified before pushing, not after.** The earlier misses were Linux-only faults a
Windows host cannot see, so the pattern was push-and-read-the-run. This time the fix was compiled
for Linux in a container first (`rust:1-bookworm`, `RUSTFLAGS=-Dwarnings`, `cargo check -p
orbistoun-llm`): clean. The lesson from 710 - a cross-platform claim needs a cross-platform build -
applied to the tooling, not just re-stated.

## State

Pushed `c15f46b` then `<path-fix>` (orbistoun) / collection bumps. CI run `35461857697` is the confirming
run. Local: Windows `cargo check -p orbistoun-llm --all-targets` clean (newest_versioned still used
there); identity scan clean. The two remaining unknowns are still only a macOS runner's to settle -
that the working-directory fix lets the x86-64 std install, and that Rosetta then runs the tests
(D706). If macOS stays red after this, the fallback is the Intel runner `macos-13`.
