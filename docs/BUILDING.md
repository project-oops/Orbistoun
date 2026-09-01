# Building orbistoun

There is one command and it is `bin/orbistoun`. Everything below is a verb on it, and every
verb is the same command CI runs - not a description of one.

```bash
./bin/orbistoun doctor --fix   # is this machine ready; --fix installs what is missing
./bin/orbistoun check          # is the tree sound
```

If those two pass, you have a working build.

## What you need

**A Rust toolchain**, and no C compiler, vendor SDK, firmware or signing keys. Nothing about a
build touches the hardware.

### One sibling

**A clone of only this repository is no longer enough.** orbistoun takes `oops-build`,
`oops-log`, `oops-paths` and `oops-docs` from `oops-libs` by relative path, as a sibling, so
the collection layout is a build requirement. Without it the build fails as a missing
*directory* rather than as a missing dependency, which is a much worse error to read.

```bash
./bin/oops bootstrap orbistoun    # fetches oops-libs, and nothing else
```

Three things said otherwise until recently, and each had been true once: the README's
"Standalone works here", the collection's `bootstrap` table, and this repository's own CI,
which checked itself out flat and so could not have built. None of them was wrong when
written.

### The toolchain

`rust-toolchain.toml` pins the build toolchain, and rustup honours it over whatever default
is already installed - including over the toolchain a CI action just set up. So there is
nothing to select and no version to match by hand.

| | |
|---|---|
| build toolchain | pinned in `rust-toolchain.toml`, with `rustfmt`, `clippy` and `rust-src` |
| MSRV floor | `rust-version` in `Cargo.toml` - a separate number, and deliberately lower |

`rust-src` is there because the conformance harness runs the host-side crates under Miri
(see [TESTING.md](TESTING.md)), which needs a local std source.

Four cargo tools are **optional**: `cargo-nextest`, `cargo-deny`, `cargo-audit`,
`cargo-machete`. Without them `check` falls back to `cargo test` and skips the audits, and it
says so in the run rather than passing quietly. `./bin/orbistoun doctor --fix` installs them.

Installing a Rust toolchain is deliberately *not* something `--fix` does. That is a
machine-wide decision belonging to the person who ran a script to ask a question.

## The seven shared verbs

Every OOPS project carries these, so `oops test orbistoun` and `./bin/orbistoun test` are one
command reached two ways.

| verb | what it does |
|---|---|
| `build` | release build of `orbistoun-cli`. Extra arguments pass through, so the release workflow's `--target <triple>` reaches the same verb a person runs |
| `test` | the test suite, under nextest when it is installed |
| `lint` | clippy at `-D warnings` |
| `fmt` | format in place |
| `check` | the full gate - see below |
| `clean` | remove build output |
| `doc` | build the API docs, without opening a browser |

## orbistoun's own verbs

Far more than the seven, because the emulator's own loop lives in this script rather than
beside it.

| verb | what it does |
|---|---|
| `run <title>` | one turn of the actual work: resolve a title, refresh names if stale, run under a time limit, report how far it got |
| `doctor [--fix]` | is this machine ready |
| `fix` | `cargo fmt` **and** `clippy --fix` |
| `cli <args...>` | the `orbistoun-cli` binary, raw |
| `docs` | build the API docs and open a browser |
| `site` | assemble the Pages bundle into `./site` for local preview |
| `sweep` | run every local guest and rank what to implement next |
| `names` | regenerate `symbols/` from local guest modules |
| `suggest [n\|benchmark] [id]` | ask a model for words; `benchmark` ranks them |
| `provenance` | no console-derived material is tracked |
| `symbols-audit` | every committed name re-derives here, or is on the ceiling |
| `constants` | the harvested ABI constants still match their headers |
| `tables` | the shader tables still match what generates them |
| `knowledge-audit` | every recorded behaviour accounts for itself |
| `prose` | no line-continued string literals |
| `decide "<title>"` | reserve the next decision number, atomically |
| `hooks` | install the pre-push gate |

Two of those are deliberately **not** folded into a shared verb. `fix` applies clippy
suggestions as well as formatting, which is a mutating operation that should be asked for by
name rather than hidden inside `fmt`. And `docs` opens a browser where `doc` does not, because
a build step that launches a browser cannot go in a pipeline.

## What `check` actually runs

In order, and it does **not stop at the first failure**: a setup problem should end the run,
but a failing gate step should not take the rest of the tree with it. Failures accumulate and
are listed at the end.

1. `provenance` - no firmware, keys, dumps or guest binaries tracked
2. `constants` - harvested ABI constants still match their headers
3. `decisions` - the decision log is well-formed
4. `prose` - no line-continued string literals
5. generated numbers still match what generates them
6. `symbols-audit` - every committed name re-derives
7. `tables` - shader tables still match their generator
8. `cargo fmt --check`
9. `cargo clippy --all-targets -- -D warnings`
10. `cargo check --all-targets`
11. the test suite, then the doctests
12. the device-dependent tests, **re-run with output shown**
13. the packet vocabulary check
14. `cargo doc` with broken intra-doc links as errors
15. the optional audits, when installed

Steps 12 and 13 are re-run rather than trusted. The Vulkan tests *skip* when there is no
device, and a test harness captures the output of a passing test - so the skip is invisible in
the run above it. The packet vocabulary is checked against captures of a real guest and there
are none yet, so that suite passes while verifying nothing. Both are reported explicitly,
because a green run must not imply a check that did not happen.

### Narrowing it

```bash
./bin/orbistoun check --only "orbistoun-submit orbistoun-cli"
```

`--only` narrows the cargo steps to those crates, for when another session has a half-written
crate elsewhere in the workspace. It prints **"passed for &lt;crates&gt;"** rather than "all checks
passed", and says in as many words that the rest of the workspace was not compiled. A subset
that passed is not a tree that is sound, and the two must never print the same word.

Note that clippy gets `--no-deps` when scoped. `-p` alone is not a scope: clippy runs on every
workspace crate it compiles from source, so scoping to `orbistoun-cli` - which depends on
nearly everything - lints the whole tree and reports somebody else's finding as yours.

## What CI runs

`.github/workflows/ci.yml`, and every job reaches through this script rather than past it:
`fmt-check`, `lint`, `compile`, `provenance`, `prose`, `knowledge-audit`, `symbols-audit`,
`security`, `test`, `doc`. The tests run on Linux, Windows and macOS.

**Local `check` is a superset of CI**, not a copy of it. It also runs `constants`,
`decisions`, `tables` and the generated-number check, and it re-runs the device tests. So a
local pass implies a CI pass; the reverse does not hold.

## Running a title

Building is not the same as having something to run. `run` needs a guest module under
`titles/`, and **nothing in that directory is ever tracked** - not now and not later. With
none present, everything that describes what orbistoun *knows* still works:

```bash
./bin/orbistoun cli symbols      # every system-library function declared
./bin/orbistoun cli questions    # everything written down that is not known, ranked
./bin/orbistoun cli worklist     # what to implement next, totalled across every run
```

[THE_LOOP.md](THE_LOOP.md) is the one-page explanation of what a turn of the work does,
including which steps still need a person.

## From the collection

[OOPS](https://github.com/project-oops/OOPS) holds all four side by side and carries one entry
point over them:

```bash
./bin/oops check orbistoun
```

That relays to this script rather than reimplementing anything, so the two cannot disagree.
[The collection's BUILDING.md](https://github.com/project-oops/OOPS/blob/main/docs/BUILDING.md)
covers the verbs that are about the collection rather than about one project - `bootstrap`,
`gates`, `all`, `git`, `status` - and the Windows and WSL handling.
