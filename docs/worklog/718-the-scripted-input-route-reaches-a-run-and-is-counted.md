# 718. The scripted-input route reaches a run, and a played script is counted

**2026-09-19** — `-02a0`'s remaining half. D704 built the deterministic input mechanism (a
file of timed pad states, sampled by `scePadReadState`, counted by the run report) but nothing
started one playing on a run: no code called `script::install`, so `poll` always saw an empty
slot. This wires the config that names a script through to a run, and demonstrates a played
script being counted on a real title. Recorded as D707.

## What was wired

- **`orbistoun_input::Source::Script { path }`** - a fourth thing that can drive a port,
  beside `Empty`/`Keyboard`/`Gamepad`. It is the only source a windowless run can drive itself:
  keyboard and gamepad are live host input and reach a run only through the GUI streaming it in,
  so a compat run could press nothing. A script is sampled in the worker as a pure function of
  run time, so it needs no window.
- **`orbistoun_service::scripted_pad`** - reads the first script-driven port's file, parses and
  validates it, failing the run on any of the three rather than entering on an input nobody
  wrote (D153). It lives in the service, not `orbistoun-input`, because that crate carries no
  format dependency by design (D704) and the service already parses the configuration's TOML.
- **`orbistoun-worker`** threads `FileConfig.pads` through `ServiceConfig`/`Service`, and
  `install_scripted_input` installs the validated script **at the entry path** - the last step
  before the jump, so the script's clock (`at_ms` is milliseconds from the start of the run) is
  the run's own rather than starting seconds early during placement and linking.

## Why it stops where it does

The played script is **counted, not delivered**. `pad_read_state` samples it and hands it to
`latest`, but still writes the at-rest bytes to the guest, because which byte carries a button
is unmeasured (D345, obSCEne `d1c4`). So the report says "N pad update(s) arrived and none
reached the guest - no measured layout to write one into", which is the honest state. When
`d1c4` lands, only that last copy changes; every script written now keeps working.

## Demonstrated, and a wrong turn worth recording

A first run under a scripted config used **PPSA02664**, and the report showed *no* pad line at
all. A probe (an unconditional counter that does not depend on `eprintln`, which is unreliable
once the guest's `fs` base is installed) proved why: PPSA02664 makes **zero** `scePadReadState`
calls - it faults at the AGC wall during graphics init, before its input loop. The `738` calls
that looked like PPSA02664's were the **corpus aggregate** across six runs, read from the wrong
section of the worklist. A title that dies before input cannot demonstrate an input route, and
that is a fact about the title, not the route.

Re-run against **PPSA99980** (the obSCEne probe, which reaches `flipped` and polls the pad), the
report shows:

```
orbistoun: shell
  0 request(s) carried out
  163 pad update(s) arrived and none reached the guest - no measured layout to write one into
```

So the whole chain is observed end to end: the configured script installs on the run path
("playing input script ... (4 step(s)) on player 1"), `pad_read_state` samples it 163 times,
and `latest::summarise` surfaces the count in the run report. Before this, a script pressing
buttons into a run produced nothing and said nothing about it.

## Tests

- `orbistoun-input`: `a_script_source_survives_the_config_round_trip` pins the config schema by
  an actual serialise/deserialise round trip (an internally-tagged enum with a struct variant is
  the shape TOML is fussiest about).
- `orbistoun-service`: `scripted_pad` is covered four ways - no scripted port reads no script; a
  named script is read relative to the config and validated; a missing file fails the run; a
  script that does not describe a run that could happen (steps out of order) fails the run, with
  `orbistoun-input`'s own refusal carried out to where a person reads it.

## What remains of -02a0

The input route is wired, tested and demonstrated. Delivering a scripted press to the guest is
gated on `d1c4` (the button encoding), per-port scripting is later work, and minute-scale runs
are a separate concern. The waiting-versus-finished outcome (`QUIET_TO_LIMIT`) was already wired
(worklog 591); a scripted run is what now makes a guest waiting on input observable rather than
silent.

## Gate state

`./bin/orbistoun check` green end to end (workspace compiles, clippy `-D warnings` clean, the new
input/service tests and the existing suite pass, docs/prose/fmt clean, identity scan clean). The
demonstration config and script file were created under the data root for the run and removed
after; `compat/` is unchanged (the scripted run's verdict was `same`, so no record moved). No
commit.
