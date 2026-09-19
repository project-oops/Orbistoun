# 708. CI tests macOS as x86_64-apple-darwin, so the sysv64 wall stops walling it

**2026-09-19** — inbox `-caf9`, slice 2 (the aarch64 ABI slice): the CI `test` job ran
`macos-latest`, which is Apple Silicon (aarch64). Guest code is x86-64 and the ABI boundary is
`extern "sysv64"` at ~28 sites across `orbistoun-abi`, `orbistoun-thunk` and `orbistoun-libc`,
plus the guest-entry inline assembly. On aarch64 `"sysv64"` is not a supported ABI, so the macOS
job did not fail a test - it failed to **compile**, `error[E0570]`, and has on every commit.

## The fix is a decision the project already made, applied to the job that missed it

There are two ways to make the macOS job pass, and `release.yml` already chose between them.
Its matrix builds the macOS artifact as `x86_64-apple-darwin` - what Rosetta runs on Apple
Silicon - and its comment says why: the sysv64 sites have no arch gate, so an aarch64 target
fails to compile, and gating them "is the other answer, and it is 28 sites of real work (D208)."
The release workflow was fixed to build the x86-64 triple; the CI `test` job was never brought
into line and kept building natively for aarch64.

So this brings the `test` job into line rather than inventing anything (D706):

- The `macos-latest` entry now carries `target: x86_64-apple-darwin`. The runner stays Apple
  Silicon; a new step does `rustup target add x86_64-apple-darwin` and exports
  `CARGO_BUILD_TARGET` into the job environment, so cargo and nextest cross-build the x86-64
  triple and Rosetta runs the resulting test binaries.
- `CARGO_BUILD_TARGET` in the environment, rather than a `--target` flag, means the project's own
  `oops test orbistoun` verb is unchanged - the same front door Linux and Windows use, which is
  what keeps CI and `./bin/orbistoun check` from drifting (the reason ci.yml routes through the
  verbs at all). The step is gated `if: matrix.target != ''`, so Linux and Windows still build
  their native host triple untouched.

## Why not gate the 28 sites

Gating every `extern "sysv64"` site and inline-asm entry behind `#[cfg(target_arch = "x86_64")]`
with honest-refusal counterparts (the pattern D208 gave `enter_process`) is real, and it was
attempted far enough to see its shape: `orbistoun-abi`'s `enter.rs` alone has three sysv64 report
callbacks, their stub-emitters, and two asm entry points, and the same boundary recurs in
`orbistoun-thunk` and `orbistoun-libc`. Its only product is an aarch64 build that compiles the
analysis-only subset and can never run a guest - which the project does not ship (release.yml
ships x86_64-apple-darwin) and principle 12 rules out a second execution backend for on purpose.
The project weighed this in release.yml and chose against it; doing it now would be spending 28
sites against a decision already made. This is the opposite of slice 1 (worklog 707), which gated
*Windows-only* code dead on the *Linux* runners - a platform's dead code, not a whole architecture
the emulator is not built for.

## What this does and does not prove

- **Removes the known blocker:** targeting x86_64-apple-darwin means the sysv64 sites compile
  (target arch is x86_64), so E0570 is gone by construction, the same way release.yml's build of
  that triple already compiles them.
- **Two things only the runner can confirm**, stated as checkable claims rather than verified
  (this is a Windows dev host, which cannot run a macOS runner): that cross-built x86-64 test
  binaries execute under Rosetta on the GitHub `macos-latest` image (the build half is already
  exercised by release.yml; the run half is the assumption), and that the full x86_64-apple-darwin
  *test* build - all crates and dev-deps, not just the CLI release binary - has no macOS-specific
  issue that was hiding behind the E0570 wall. The first green or red macOS `test` run answers
  both. If Rosetta execution proves unavailable, the fallback is an Intel runner (`macos-13`),
  which tests the same triple natively (D706).

## Gate state

The change is CI-only: `.github/workflows/ci.yml` and a new decision `D706`. No crate changed, so
the local Windows gates are unaffected; `./bin/orbistoun` decisions/prose checks and the identity
scan are run against the new files. No commit. caf9 slices 3 (cargo-machete) and 4 (the symbol
audit) remain.
