# D707 - scripted pad input is a config source, read from a file and installed at entry

**Status:** assumed
**Date:** 2026-09-19

## Context

D704 built the deterministic input mechanism: a script of timed pad states
(`orbistoun-input/src/script.rs`), sampled as a pure function of run time, that
`scePadReadState` already polls (`script::poll` → `latest::arrived`) and the run report
already counts (`latest::summarise`). Every piece existed **except the one that starts a
script playing on a run** - nothing called `script::install`, so `poll` always saw an empty
slot and no run ever played a script (inbox `REQ-20260915T0929Z-02a0`).

Two questions had to be answered to wire it: where a run *names* the script it should play,
and *when* the script starts.

## Where: a controller source, not an environment diagnostic

A run's controllers live in `FileConfig.pads` (`orbistoun_input::Pads`) - "how many
controllers there are, **what drives each**". A `Source` was `Empty`, `Keyboard`, or
`Gamepad`. A scripted pad is a fourth thing that drives a port, so it is a fourth `Source`
variant, `Script { path }`, not a new mechanism bolted on elsewhere.

It is deliberately **not** an environment variable beside `ORBISTOUN_RETURN` and the other
diagnostics. `orbistoun-env` draws the line itself: a *setting* configures how the emulator
behaves and may be persisted; a *diagnostic* changes the program to learn something and may
never come from a file. A scripted controller is a setting - it is a controller, played
deterministically - so it belongs in the configuration file, where a compat run can carry
one per title, and not among the one-shot diagnostics.

It is **file-based** (`path`) rather than inlined in the config. `02a0` asks for exactly "a
file of timed pad states read by the worker", and a script is a reusable, diffable artifact
that does not belong cluttering `config.toml`. `orbistoun-input` carries no format
dependency by design (D704), so `orbistoun-service` - which owns the configuration and
already parses TOML - reads and validates the file (`scripted_pad`), and a path is resolved
relative to the configuration's own directory.

## When: at guest entry, not at process start

`script::install` records the clock the script's `at_ms` counts from. The worker reads its
configuration at process start but does not enter the guest until it has placed, linked and
relocated the title - seconds later for a large one. Installing at process start would make
`at_ms = 500` fire half a second into *loading*, not into the run. So the script is installed
on the entry path, the last step before the jump (`install_scripted_input`, beside
`install_limits`), where its clock is the run's own.

This costs nothing today - the sampled state is counted but not delivered to the guest,
because which byte carries a button is still unmeasured (D345, obSCEne `d1c4`) - but it is
right for the moment `d1c4` lands and the timing starts to matter, and a shortcut that
constrained that is the thing principle 11 rules out.

## Decision

- `orbistoun_input::Source` gains `Script { path: String }`: a port driven by a recorded
  file of timed pad states. It is the only source the headless worker can drive itself;
  `Keyboard` and `Gamepad` are live host input and reach a run only through the GUI, so a
  windowless compat run can press nothing without this.
- `orbistoun_service::scripted_pad` reads the first script-driven port's file relative to the
  configuration, parses it, and validates it, failing the run on any of the three rather than
  entering on an input nobody wrote (D153).
- `FileConfig.pads` is threaded to the worker through `ServiceConfig`/`Service`, and
  `orbistoun-worker`'s `install_scripted_input` installs the validated script at the entry
  path.

## What this is not

It does not deliver a scripted press to the guest - the encoding block (D345, `d1c4`) is
unchanged, so a played script is *counted* ("N pad update(s) arrived and none reached the
guest"), not read. It drives player 1 (port 0): the sampled state is handed to `latest` as
the whole port table, which is where `pad_read_state` routes it. Per-port scripting and a
delivered press are later work, gated on `d1c4`, not on this.

## Consequences

Demonstrated end to end: with a scripted pad configured, PPSA99980 (which polls the pad)
reported "163 pad update(s) arrived and none reached the guest", where before a played script
produced nothing and said nothing about it. A title that faults before its input loop
(PPSA02664, at the AGC wall) reports no pad updates, which is the honest difference between a
run that reached input and one that did not.
