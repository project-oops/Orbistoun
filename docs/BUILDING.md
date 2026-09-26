# Building orbistoun

`bin/orbistoun` is the one command. Everything below is a verb on it, and each verb is the
command CI runs.

```bash
./bin/orbistoun doctor --fix   # is this machine ready; --fix installs what is missing
./bin/orbistoun check          # is the tree sound
```

When both pass, the build is working.

## Requirements

**A Rust toolchain.** No C compiler, vendor SDK, firmware or signing keys; a build never
touches the hardware.

### The sibling repository

orbistoun takes `oops-build`, `oops-log`, `oops-paths` and `oops-docs` from `oops-libs` by
relative path, so the collection layout is a build requirement. Without the sibling, the
build fails on a missing directory rather than a missing dependency.

```bash
./bin/oops bootstrap orbistoun    # fetches oops-libs, and nothing else
```

CI checks out the collection the same way.

### The toolchain

`rust-toolchain.toml` pins the build toolchain, and rustup honours it over any installed
default, including one a CI action sets up. There is nothing to select by hand.

| | |
|---|---|
| build toolchain | pinned in `rust-toolchain.toml`, with `rustfmt`, `clippy` and `rust-src` |
| MSRV floor | `rust-version` in `Cargo.toml`, a separate and lower number |

`rust-src` is included for running host-side crates under Miri, which needs a local std
source.

Four cargo tools are optional: `cargo-nextest`, `cargo-deny`, `cargo-audit` and
`cargo-machete`. Without them `check` falls back to `cargo test`, skips the audits, and says
so in its output. `./bin/orbistoun doctor --fix` installs them. It does not install a Rust
toolchain: that is a machine-wide decision left to the person running it.

## The shared verbs

Every OOPS project carries these seven, so `oops test orbistoun` and `./bin/orbistoun test` are
one command reached two ways.

| Verb | What it does |
|---|---|
| `build` | release build of `orbistoun-cli`. Extra arguments pass through, so the release workflow's `--target <triple>` reaches the same verb |
| `test` | the test suite, under nextest when it is installed |
| `lint` | clippy at `-D warnings` |
| `fmt` | format in place |
| `check` | the full gate, below |
| `clean` | remove build output |
| `doc` | build the API docs, without opening a browser |

## orbistoun's own verbs

The emulator's working loop lives in this script as well.

| Verb | What it does |
|---|---|
| `run <title>` | boot one title: resolve it, refresh names if stale, run under a time limit, report how far it got |
| `turn <title>` | one turn of the loop with nobody reading the findings |
| `doctor [--fix]` | is this machine ready |
| `fix` | `cargo fmt` and `clippy --fix` |
| `cli <args...>` | the `orbistoun-cli` binary, raw |
| `docs` | build the API docs and open a browser |
| `site` | assemble the Pages bundle into `./site` for local preview |
| `sweep` | run every local guest and rank what to implement next |
| `names` | regenerate `symbols/` from local guest modules |
| `suggest [n\|benchmark] [id]` | ask a model for words; `benchmark` ranks them |
| `fmt-check` | formatting, checked rather than applied |
| `compile` | `cargo check --workspace --all-targets` |
| `security` | `cargo audit` and `cargo deny check` |
| `provenance` | no firmware, keys, dumps or guest binaries are tracked |
| `symbols-audit` | every committed name re-derives here, or is on the ceiling |
| `knowledge-audit` | every recorded behaviour accounts for itself |
| `constants` | the harvested ABI constants still match their headers |
| `tables` | the shader tables still match what generates them |
| `prose` | no line-continued string literals |
| `decisions` | decision numbers are unique and indexed |
| `decide "<title>"` | reserve the next decision number, atomically |
| `worklog "<title>"` | reserve the next worklog number, atomically |
| `hooks` | install the pre-push gate and the tools it needs |

`fix` is separate from `fmt` because it applies clippy suggestions, a mutating operation that
is asked for by name. `docs` is separate from `doc` because it opens a browser, which a
pipeline step cannot do.

## The gate

`check` runs these steps in order. A setup problem ends the run; a failing gate step does not.
Failures accumulate and are listed at the end.

1. `provenance`: no firmware, keys, dumps or guest binaries tracked
2. `constants`: harvested ABI constants still match their headers
3. `decisions`: decision numbers are unique and indexed
4. worklog numbers are unique
5. `prose`: no line-continued string literals
6. the generated numbers blocks are current (`orbistoun-cli status --check`)
7. `symbols-audit`: every committed name re-derives
8. `tables`: shader tables still match their generator
9. measured hardware behaviour is asserted or declared outstanding
10. the committed differential reference run matches the reference program
11. `cargo fmt --check`
12. `cargo clippy --all-targets -- -D warnings`
13. `cargo check --all-targets`
14. the test suite, then the doctests
15. the device-dependent Vulkan tests, re-run with output shown
16. the packet vocabulary check
17. `cargo doc` with broken intra-doc links as errors
18. the optional audits, when installed (advisory)

Steps 15 and 16 report whether they verified anything. The Vulkan tests skip when there is no
device, and a test harness hides the output of a passing test, so the re-run prints either
that the tests executed against a device or which ones were skipped. The packet vocabulary is
checked against captured command streams, and the step warns when there are none. A green run
does not imply a check that did not happen.

### Narrowing the gate

```bash
./bin/orbistoun check --only "orbistoun-submit orbistoun-cli"
```

`--only` narrows the cargo steps to the named crates, for when another crate in the workspace
is mid-edit. A scoped run prints `passed for <crates>` instead of `all checks passed` and states
that the rest of the workspace was not compiled, linted or tested (D319). Clippy gets
`--no-deps` when scoped, because `-p` alone still lints every workspace crate it compiles.
`--only` is accepted only by `check`.

## CI

`.github/workflows/ci.yml` reaches every step through this script:

| Job | Runs |
|---|---|
| Check + fmt + clippy | `fmt-check`, `lint`, `compile` |
| Provenance guard | `provenance`, `prose` |
| Knowledge provenance audit | `knowledge-audit` |
| Symbol provenance audit | `symbols-audit` |
| Audit + deny + machete | `security` |
| Test | `oops test orbistoun` on Linux, Windows and macOS |
| Rustdoc | the API docs |

Local `check` also runs `constants`, `decisions`, the worklog and generated-number checks,
`tables`, the hardware and differential checks, and the device-test re-run. `knowledge-audit`
runs in CI and by its own verb, not inside `check`.

## Running a title

`run` needs a title in the title library: the shared `titles/` under the data directory
(`orbistoun-cli paths` prints it), outside this repository. Without one, the commands that
describe what orbistoun knows still run:

```bash
./bin/orbistoun cli symbols      # every system-library function declared
./bin/orbistoun cli questions    # everything written down that is not known, ranked
./bin/orbistoun cli worklist     # what to implement next, totalled across every run
```

[THE_LOOP.md](THE_LOOP.md) explains what a turn of the work does.

## From the collection

The [OOPS](https://github.com/project-oops/OOPS) collection holds the projects side by side
with one entry point over them:

```bash
./bin/oops check orbistoun
```

It relays to this script, so the two cannot disagree.
[The collection's BUILDING.md](https://github.com/project-oops/OOPS/blob/main/docs/BUILDING.md)
covers the collection-level verbs (`bootstrap`, `gates`, `all`, `git`, `status`) and the
Windows and WSL handling.
