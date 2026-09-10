# 497. The comparison ran on the wrong machine

**2026-09-10** - continuing 496, in parallel with a separate session working in the loader

The loader fix for `900-surface/control` went to another session with its own prompt, because
the loader directory is denied to this one. This session took what was reachable without
touching its files.

## `130-layout` was a methodology gap, not a code gap

`130-layout/system-software-version` read `partial` against a hardware leg that passes it. The
implementation was already right (D420): measured layout, profile-carried value, and a refusal on
a machine that carries none. That refusal is exactly what every run this session got.
No `shell.toml` exists and `bin/orbistoun` never passes `--profile`, so every conformance
comparison presented an **unset** machine against a console that reports
`firmware|known|12.40`.

Under `--profile prospero-cex-12.40` exactly one row moved, and it was that one: `partial` to
`pass`.

## Surprises

**Presenting the reference machine moved no title.** All six retail titles re-run under the
profile landed exactly where they had before: same imports, same stops, calls within ±18. None
calls `sceKernelGetSystemSwVersion`. That is a measured reason to keep the unset machine as the
sweep default rather than an argued one.

**obSCEne's explanation of `13.090.001` does not survive its own data.** `sysinfo.c` says the
call answers the PS4-compatibility environment's version *"because the title is a `ps4_game`"*.
The identical 40 bytes come back on the eboot leg, which is not a package, and on both payload
legs, which are not titles at all. D420 stands. What a `ps5-native` title reads is still
unmeasured, because none of the 88 hardware logs is a `ps5-native` leg. Filed as
`REQ-20260910T0900Z-7e31`.

**`135-sysctl/names` is partial on hardware too.** The console answers 12 of its 19 names. Filling
in orbistoun's six missing names matches the console name for name and flips nothing, so the
change is recorded as fidelity rather than as a win. Two of the six are published and belong to
the platform. Three are one console's values and belong to its profile. One is live state and
stays refused (D675).

**Unset knobs refuse rather than answer empty**, deliberately unlike D447's `kern.osrelease`.
Extending D447 would have the default machine score knobs it knows nothing about.

**The corpus multi-source fetch was never wired.** Asked where it went: `Origin::classify` and
`first_that_answers` are committed in `orbistoun-corpus` and called only by their own tests.
`Source::sync` reads `repo`/`tag`/`path` and never `sources`. The four obSCEne entries in
`corpus/sources.toml` have `sources` but no `path` and no assets, so `corpus sync` prints their
names and fetches nothing, without an error. Meanwhile the manifest's comment claims *"`sync`
tries them in order"*. Two more pieces are missing too: nothing in the repository unpacks the
`.zip` release origins, and the local origins for those two are directories, which `sync` cannot
read. Not changed. Reported.

**`category::present` is behaviour-neutral for the whole corpus.** Every title declares
`applicationCategoryType` `0`, which is BigApp and already the default `presented()` answers.
Wiring it would make the SystemApp and Daemon rules reachable and change no title. Still deferred.

**Another session wrote into D675.** The number was reserved here. Its placeholder was later
filled with a short summary of this change, citing D010 where principle 3 was meant, most likely
by the loader session clearing a gate that refuses a reserved-but-empty record. It has been
replaced with the full reasoning, keeping the platform/machine split that draft got right. Two
sessions sharing one working tree and one gate is a coordination hazard worth naming before it
does something worse than fill in a paragraph.

## Verified against the guest

Under `prospero-cex-12.40`, obSCEne's header now reads `firmware|known|12.40`, matching the
console's, and `135-sysctl/names` answers 11 names against the console's 12. The five new knobs
match the console's bytes exactly. The name still missing is `hw.availpages`, refused on purpose.
The unset machine answers 8: the two published knobs, with the three profile knobs refused. The
tables are in D675.

## Two inbox items from oops-libs, and a proposal back

- **`PORTABLE_NOTE` re-export (REQ ...0825Z-d31e), done.** `orbistoun-paths` now
  `pub use oops_paths::PORTABLE_NOTE` instead of declaring its own `"PORTABLE.txt"` - the note
  body stays orbistoun's, only the shared filename is delegated. 13 portable-mode tests green,
  clippy and fmt clean. **Not marked resolved in the inbox**: its acceptance wants a commit, and
  this session commits nothing unless asked, so it sits done-in-tree rather than closed.
- **GPU-capture glue (REQ ...0822Z-26aa) left OPEN.** A real three-piece feature - a dump into
  `packet::walk`, a per-draw shader/descriptor correlation record, and a path from a capture to
  `orbistoun shaders`. obSCEne's `170-gpu-capture` emits the dumps as `OBS|bytes` windows, not a
  `.bin`, so piece 1 is a reader over those. Not started: it is a substantial feature and the
  workspace does not currently compile (see below), so half-building it would be unverifiable.

- **Filed the multi-source convergence to oops-libs (REQ ...1010Z-9c22).** The operator flagged
  that Prosperous built its own ordered-source fetch. It is the same shape as orbistoun's, and
  the shareable part is the *pure policy* - `Origin::classify` and the ordered verify-before-keep
  walker - not the fetcher: orbistoun links `reqwest`, Prosperous deliberately shells out to a
  download command to avoid a TLS stack, each with a written rationale. Orbistoun's own
  `first_that_answers`/`Origin` are that policy already and are **not wired** into `sync`, so
  adopting a shared `oops-fetch` would be the wiring it never finished. Proposed, not built - a
  three-repo change is oops-libs' to shape.

## The tree would not compile, and it was not this session

`./bin/orbistoun check` failed on one thing: `unresolved import orbistoun_elf::dynamic::Binding`
in `orbistoun-service`'s tests. That is the **other session's** in-flight D676 (weak-symbol
binding) mid-edit - a `Binding` type used before it is exported. None of this session's crates
touch it: `orbistoun-core`, `orbistoun-shell`, `orbistoun-libc` and `orbistoun-paths` all pass
in isolation (47 + 130 + 39 + 13 tests, clippy clean). Recorded so a later reader does not
attribute the red to D675. It is the shared-working-tree hazard this file already named, now
having actually stopped a gate.

**Resolved by the other session shortly after.** It finished D676 - the `Binding` type is
exported and wired, `orbistoun-service` compiles, and the absolute-path identity leak it had
introduced in `orbistoun-elf/tests/static_symbols.rs` is gone (guard exit 0). Verified from here:
`900-surface/control` now **passes on `obscene-payload`** (`obs_census_control_absent` binds to
zero and reads absent), which is the binary that preserves `STB_WEAK`. It still fails on
`PPSA99980`, and D676 explains why - SELFish's `mkmodule` force-binds every import `GLOBAL`, so a
packaged title carries no weak symbols at all. That tension is filed to SELFish as
REQ-20260910T1105Z-5d20: preserve source binding, or confirm all-GLOBAL is a platform fact and the
packaged census control is payload-only by construction.

## Next

- `oops-fetch` adoption, if oops-libs takes REQ ...1010Z-9c22 - and loop Prosperous in then.
- The GPU-capture reader (26aa), once the tree compiles again.
- `category::present`, for reachability rather than for any title.
- Whatever the loader session's D676 leaves for `111-modlink/walk`.
