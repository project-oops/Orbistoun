# D706 - CI tests macOS as x86_64-apple-darwin, matching release.yml, not by gating 28 sysv64 sites

**Status:** assumed
**Date:** 2026-09-19

## Context

The CI `test` matrix ran `macos-latest`, which is Apple Silicon (aarch64). Guest code is
x86-64 and runs natively; the ABI boundary is declared `extern "sysv64"` at ~28 sites across
`orbistoun-abi`, `orbistoun-thunk` and `orbistoun-libc`, plus the guest-entry inline assembly.
On aarch64 `"sysv64"` is not a supported ABI, so the macOS job did not fail a test - it failed
to **compile**, with `error[E0570]`. That job has been red on every commit, which is one of the
four structural reasons orbistoun's CI is red (inbox `-caf9`).

## The two answers, and which the project already chose

There are exactly two ways to make the macOS job pass:

1. **Gate the ABI off aarch64.** Put every `extern "sysv64"` site, every x86 inline-asm entry,
   and every stub-emitting helper behind `#[cfg(target_arch = "x86_64")]`, with a
   `#[cfg(not(target_arch = "x86_64"))]` counterpart that honestly refuses (the pattern D208
   already applied to `enter_process`). This makes an aarch64 build compile the analysis-only
   subset - container parsing, symbol naming, trace reading - which can never run a guest.

2. **Build the x86-64 target on the macOS runner.** Guest execution is x86-64 by architecture
   (principle 12 rules out a second execution backend on purpose), so the macOS artifact orbistoun
   actually ships is `x86_64-apple-darwin` - what Rosetta runs on Apple Silicon.

**`release.yml` already chose (2).** Its matrix builds `x86_64-apple-darwin`, and its comment
states the reasoning outright: the sysv64 sites have no arch gate, so Apple Silicon fails to
compile; gating "is the other answer, and it is 28 sites of real work (D208)." The release
workflow was fixed; the CI `test` job was simply never brought into line with it, and kept
building natively for aarch64.

## Decision

Bring the CI `test` job into line with `release.yml`: the `macos-latest` entry builds and tests
`x86_64-apple-darwin`. The runner stays Apple Silicon (`macos-latest`); the target is cross-built
and the x86-64 test binaries execute under Rosetta 2, which GitHub's macOS runners provide. This
is done with `rustup target add x86_64-apple-darwin` and `CARGO_BUILD_TARGET`, so the project's
own `oops test orbistoun` verb is unchanged - the same front door the other two OSes use, which
is what keeps CI and `./bin/orbistoun check` from drifting.

Gating the 28 sites (answer 1) is **not** done, for the reason release.yml already gives: it is
real work whose only product is an aarch64 build the project does not ship and cannot execute a
guest on. The dead-code slice of `-caf9` (worklog 707) is a different thing - that gated
*Windows-only* code that was dead on the *Linux* runners; this is about a whole *architecture*
the emulator is not built for.

## Why this is `assumed`

Made without input, on a Windows dev host that cannot run a macOS runner. The one claim it rests
on that this machine cannot check is that cross-compiled `x86_64-apple-darwin` test binaries
execute under Rosetta on the GitHub `macos-latest` image - stated as a checkable claim rather
than a verified one (principle 3). The build half is already exercised by `release.yml`; the
first green (or red) macOS `test` run confirms or refutes the run half. If Rosetta execution
proves unavailable in that context, the fallback is an Intel runner (`macos-13`), which tests the
same triple natively.

## Consequences

- The macOS CI job compiles (the E0570 wall is gone) and tests the exact triple release ships.
- `enter_process`'s existing aarch64 refusal (D208) and the `can_execute_guests()` reporter stay
  as they are - they remain correct for anyone who builds an aarch64 binary by hand, and this
  decision simply means CI does not.
- If orbistoun ever wants CI to guard the analysis-only aarch64 build as a supported
  configuration, answer 1 becomes worth its 28 sites - but that is a new want with its own
  measurement, not this.
