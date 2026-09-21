# 756. Error-dialog init is served, and the stale surfaces are regenerated

**2026-09-21** — a fresh title after PPSA02664's wall was parked. PPSA28061 is close — 323 of 326 calls
answered, three on stubs — and its wall is one clear import. Filling one of the other two stubs
(`sceErrorDialogInitialize`) is a small, honest API implementation that costs nothing to get right, and
carrying it through `SERVES_NOTHING` swept up the generated-surface drift that had reddened the gate for
thirty worklogs. The title's actual wall is named and left for a probe, because it is a measurement, not
a guess.

## PPSA28061's wall is a measurable export, not a non-export like PPSA02664's

The run gives up at `image+0x10b9e9`, a `read of 0xf7ff0039` — one of our own placeholder codes used as
an address. The call just before is `libSceAgc::sceAgcGetRegisterDefaults2(0xd) -> 0xf7ff0001`:
`sceAgcGetRegisterDefaults2` is declared (arity 6) with **no handler**, so it answered the loud
placeholder, and the guest dereferenced that as the pointer-to-defaults it returns (`+0x38` off it).
The fix is to answer a real defaults block. Unlike PPSA02664's phantom, this is a **named export** —
obSCEne can measure it — so the honest path is a probe of what `sceAgcGetRegisterDefaults2(0xd)` returns
and what its block holds, then an implementation from that, not a guessed table. Filed as the next step;
not implemented here, because a guessed defaults block is the hack the standing instruction forbids.

## What was served: the honest error-dialog init

The second stub, `sceErrorDialogInitialize`, is a different shape and a clean fill. It is a subsystem
init, and an SDK `*Initialize` answers `OK` (`0x0`) on its first call. That is not an unmeasured value
in the sense `sceUserServiceGetAgeLevel` is (D346) — it is the near-universal init convention, and the
project already retired `libSceAudioOut` from `SERVES_NOTHING` on exactly this reasoning ("setting a
subsystem up is not claiming a sound was made"). So the handler answers `OK`, writes nothing, guards no
argument (`error_dialog_initialize`, `orbistoun-systemservice`), the knowledge records it as
`known_by: assumed` with the retire-against-a-probe note, and `libSceErrorDialog` leaves
`SERVES_NOTHING`. It is not PPSA28061's wall — that is `sceAgcGetRegisterDefaults2` — but it is a real
API the console serves, answered the way the console serves it.

## The drift that came off with it

`SERVES_NOTHING` is enforced both directions: a library gaining an implementation fails until its entry
is deleted. Deleting `libSceErrorDialog`'s entry drops the count to `148 across 22 libraries`, which
`status --write` then wrote into `README.md` and `docs/PROJECT_STATUS.md`. Regenerating those surfaced
that they were **already stale** against a prior session's uncommitted work (declared/implemented
`967/765 -> 970/769`, recorded behaviours `815 -> 819`), so the regen folded that in too, and the
frontier golden was reblessed (`UPDATE_FRONTIER=1`). All three generated-doc gates now pass —
`status --check`, `compat markdown --check` (36 titles current), and the `frontier` test — clearing the
red that thirty worklogs carried and that the request inbox tracked as 516b/ca0d. The gate is green on
this change but for nothing.

## Gate state

Changed `crates/orbistoun-systemservice/src/lib.rs` (handler + registration + declaration-test chain +
test), `crates/orbistoun-service/src/symbols.rs` (`SERVES_NOTHING` entry removed, count comment),
`crates/orbistoun-hle/data/knowledge/libSceErrorDialog.toml`, `error_dialog.rs` (module comment), and
the regenerated `README.md`/`PROJECT_STATUS.md`/`compat-frontier.txt`.

With the generated-doc reds cleared, the full `./bin/orbistoun check` ran to its **prose** step for the
first time in thirty worklogs and caught a latent offender there: a `\`-continued string literal in
`copy_source_lead` (`crates/orbistoun-report/src/diagnose.rs`), added in worklog 741 and masked until now
by the drift reds. Converted it to `concat!()` of one-line literals with explicit named args (inline
`{src}` capture does not survive `concat!()`), the rendered text unchanged and its test still passing.
The full gate then reports **`all checks passed`** — the first fully-green `./bin/orbistoun check` since
the drift set in. `cargo fmt --check` and `cargo clippy --all-targets` clean throughout. Identity scan
clean. No commit.
