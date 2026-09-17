# 664. The prose gate caught two `\`-continued strings I introduced in tiling.rs

**2026-09-17** — auditing inbox `-d529` (the prose line-continuation gate, D184/D199) surfaced two
`\`-continued string literals I had authored this session in `crates/orbistoun-gpu/src/tiling.rs`'s
`Display` impls — my per-tick gates (clippy/fmt/test) never ran the prose gate, so they slipped
through. Fixed both; the working-tree prose gate is green again.

## What offended

Two `write!` format strings in `tiling.rs` had been broken across lines with a trailing `\`:

- `DetileError::SurfaceExceedsBlock`'s `Display` (worklog 653) —
  `"surface {width}x{height} exceeds one 64KB block ..."`
- `SurfaceError::OutsideWindow`'s `Display` (worklog 658) —
  `"surface base {base:#x} is outside the guest window at {window_base:#x} ({window_words} words)"`

`cargo fmt` collapses a `\`-continued literal by baking the source indentation into the rendered
text as a run of spaces, which is why D199 refuses them. Both are now single-line strings — single
line rather than `concat!`, because these carry implicit `{name}` captures and `concat!` defeats
capture (it would force every arg positional). The messages are unchanged.

## Why my gates missed it

`cargo clippy`/`fmt`/`test` per crate do **not** run the prose gate; only `./bin/orbistoun prose`
(and `./bin/orbistoun check`, which includes it) does. My per-tick gate routine ran the first three
and not the fourth, so a `\`-continued string passes every check I was running and fails CI. The gate
routine now includes the prose gate whenever a change touches a Rust multi-line string literal
(recorded as a standing lesson).

## Gate state

`./bin/orbistoun prose` exit 0 — "none added; 0 file(s) on the known ceiling", and no literal carries
a raw newline. `cargo fmt` clean, `cargo clippy -p orbistoun-gpu --all-targets` clean, gpu lib 73
passed. Source-only change to two `Display` strings. No commit.

## d529 is closer but still commit-blocked

While here I measured the other gates `-d529`'s last three updates named (all in other sessions'
files): the specific offenders are gone from source — `mount.rs:187`'s `&std::path::Path` no longer
exists, and `input/src/lib.rs:33` / `report/src/trace.rs:138` now use plain code spans rather than
`[…]` intra-doc links. That is a source grep, not a workspace `cargo doc`/`clippy` build, so it is not
a full-gate pass. `-d529`'s acceptance is a committed, CI-green Provenance guard, and the commit is
the operator's — so `-d529` stays OPEN, not resolvable from here.
