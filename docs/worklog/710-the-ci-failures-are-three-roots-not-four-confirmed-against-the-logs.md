# 710. The red CI is three root causes, not four - confirmed against the run logs

**2026-09-19** — inbox `-caf9`: caf9 listed four causes of orbistoun's red CI and named the fourth
"the symbol-provenance audit (`symbols/generated.json` re-derivation)." Reading the actual failing
job logs (`gh run view 35454561875`, commit `35453bb`) shows the fourth is not a symbol problem at
all - it is the *first* cause wearing another job's name. The four slices are really three roots.

## What each failing job actually died on

Nine jobs, six red. The `--json jobs` conclusions and the failed-step logs:

| Failing job | What the log shows | Root |
|---|---|---|
| Check + fmt + clippy | `error: field process_id is never read`, `static REPORTED is never used`, … | dead_code (slice 1) |
| Test (ubuntu-latest) | the same dead_code set | dead_code (slice 1) |
| Rustdoc (`cargo doc`) | the same dead_code set | dead_code (slice 1) |
| Symbol provenance audit | the same dead_code set - `./bin/orbistoun symbols-audit` builds the CLI, so it compiles the workspace `-D warnings` and dies **before the audit runs** | dead_code (slice 1) |
| Test (macos-latest) | `error[E0570]: "sysv64" is not a supported ABI` | aarch64 ABI (slice 2) |
| Audit + deny + machete | `cargo machete` unused deps | machete (slice 3) |

The three green jobs confirm the reading: **Provenance guard** compiles nothing; **Test
(windows-latest)** compiles the host-only code so none of it is dead there; **Knowledge provenance
audit** runs `cargo test -p orbistoun-hle`, which never touches `orbistoun-worker`, so the
dead_code cannot reach it - and it was green all along, which is why caf9 did not list it.

## The correction

- **caf9 "slice 4" is slice 1.** The symbol-provenance *job* is red because the crate it must
  compile to run the audit has host-only code that is dead on Linux - the exact failure worklog
  707 gated. There is no `symbols/generated.json` re-derivation problem: `./bin/orbistoun
  symbols-audit` passes locally end-to-end ("every committed name is accounted for, or on the
  ceiling"), and it fails on CI only because it never gets to run. Fixing slice 1 fixes this job;
  there is no separate slice 4 to do.
- **Rustdoc was red and caf9 did not list it** - also purely slice 1. Verified that the slice-1
  gating does not itself break rustdoc: `RUSTDOCFLAGS="-D warnings" cargo doc --target
  x86_64-unknown-linux-gnu -p orbistoun-worker --no-deps` is clean, so gating the functions
  introduced no broken intra-doc link from a comment that survived on Linux.

## Where caf9 now stands

Every one of the six red jobs is covered by a root cause that is fixed and locally verified:

- **slice 1** (worklog 707) - the `orbistoun-worker` dead_code, gated `#[cfg(windows)]`. Clears
  Check+fmt+clippy, Test(ubuntu), Rustdoc, and Symbol-provenance.
- **slice 2** (worklog 708, D706) - the macOS job builds `x86_64-apple-darwin`. Clears Test(macos).
- **slice 3** (worklog 709) - three unused deps removed. Clears Audit+deny+machete.

What is **not** done, and cannot be from here: the fixes are uncommitted, and CI grades a pushed
commit. caf9's acceptance is "the CI workflow green on a commit," which needs a commit+push (the
operator's, not the loop's) and then a run to confirm - including the two things only a macOS
runner can settle (that Rosetta executes the cross-built x86-64 tests, and that the full
x86_64-apple-darwin *test* build has no macOS issue that was hiding behind the old E0570 wall;
D706). Until that run is green, caf9 stays open - but there is no remaining *code* to write for it.

## Gate state

No tree change - this is a reading of the CI logs and a verification (`cargo doc` Linux target,
`symbols-audit` local). The slice-1/2/3 changes and their gates stand as their own worklogs record.
No commit.
