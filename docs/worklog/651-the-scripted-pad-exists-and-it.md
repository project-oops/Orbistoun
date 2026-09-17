# 651. The scripted pad exists, and it deliberately stops short of the byte it would be written into

**2026-09-17** - orbistoun-input, part of `REQ-20260915T0929Z-02a0`, recorded as D704

`02a0` says titles gate on input - a licence page, a language list, a press-to-start - and that a
guest stopped at one of those records the same outcome as a probe that ran clean, so the
compatibility table cannot tell them apart. Nothing in the tree could press a button, so no run
had ever got past a prompt.

`crates/orbistoun-input/src/script.rs` is the deterministic half: a file of timed pad states, so
a run that has to get past a prompt is repeatable. Same script, same guest, same run.

## The one real design question, and why it went the way it did

**Which byte a title reads to see a button is not known**, and this is the third module to run
into that. The 120-byte extent and its at-rest contents are measured; which offset inside carries
the buttons is an inference from a single at-rest image (D345). The measurement that would settle
it - obSCEne `REQ-20260910T0650Z-d1c4` - has been open since 2026-09-10 and is `pending` in every
sweep since, because it needs somebody physically holding a button on a console with a controller
attached. Tonight's sweeps still say `no button was seen in the window`.

So the choice was: wait for that, or find the part that does not depend on it.

**The script carries `PadState` - typed buttons, sticks, triggers - and never bytes.** The
mechanism is ours and testable today; the encoding is a measurement nobody has taken. Separated,
every script written now keeps working unaltered when the encoding lands, because what changes
then is `pad.rs`. Mixed, the route would wait on a person in a room with a controller, and
inferred offsets would be baked into a file format that could not then change.

## What a step means

A step **sets** the pad and it holds until the next one - a level, not an event queue. That
follows `latest.rs`, which keeps the most recent state per port for the reason recorded there: a
title asks what the pad is doing *now*, and replaying finished presses is worse than nothing. A
pad has no events; it has a position, sampled.

So a press and its release are two steps and the gap is the hold. A press with no release is held
to the end of the run, which is a real thing to want and therefore not an error.

The boundary is `<=`: a step takes effect at the instant it names, and the millisecond before
belongs to the step before it. The test checks 99, 100, 299 and 300 rather than the middle of
each span, because an implementation using `<` passes a mid-span check and fails at the edges -
which it does, when the comparison is flipped.

## Refused rather than sorted

Steps out of order are an error, including two at the same instant - ambiguous rather than merely
misordered. Sorting would run something other than what the file says while looking like it
worked, and both shapes are watched failing.

Ranges are checked per axis and the two axes differ: a stick is bipolar and a trigger is not, so
`-0.5` is ordinary for one and impossible for the other. A check using one range for both passes
the stick case and fails the trigger one, so both are in the test.

## A small note on the float assertions

The analogue test compares **bits**, not floats within an epsilon. These values are carried
through unchanged, so *exactly unchanged* is the property; an epsilon would also pass for an
implementation that quietly rescaled them. Clippy denies exact float comparison and is right to
in general - comparing `to_bits` says what is meant and satisfies it honestly.

## The block is now counted instead of invisible

The transport in `latest.rs` was dormant: nothing called `arrived`, nothing called `port`, so
`ARRIVED` and `READ` both sat at zero and `summarise()` - which already knew how to say *"n pad
update(s) arrived and none reached the guest - no measured layout to write one into"* - was never
reached. The script is the producer it was missing.

`pad_read_state` now samples the installed script and hands the state to `latest`, then writes
the at-rest bytes exactly as before, because it still cannot do better. Sampling happens at the
moment the guest asks rather than on a timer, which is what keeps a run repeatable: the state is
a pure function of elapsed time, so there is no feeder thread to race and no tick rate to drift
against the guest's own polling.

What that buys is the difference between a block nobody can see and one that is counted. A run
whose script presses a button four hundred times currently produces nothing at all and says
nothing about it; now the counters move and the report line has something to report. When
`d1c4` lands, the only thing that changes is the copy at the end of `pad_read_state`.

## Files

- `crates/orbistoun-input/src/script.rs` - the format, the reader, the installed script, and
  eight tests.
- `crates/orbistoun-input/src/lib.rs` - the module, and the sample in `pad_read_state`.
- `docs/decisions/D704-a-scripted-pad-is-a-level-held-until.md`.

## Next

**Nothing installs a script yet.** The shim samples one when it is there and the counters record
what could not be delivered, but no run path calls `install`, so no compatibility run plays a
script. That is the rest of `02a0` - the worker choosing a script, minute-scale runs, and an
outcome separating a waiting guest from a finished one - and it touches `orbistoun-worker`.

`latest::summarise()` is also still unwired: it produces the line and `orbistoun-worker`'s report
does not call it. Worth doing in the same change, since until then the counters move where nobody
reads them.

And the guest-visible half stays blocked on obSCEne `REQ-20260910T0650Z-d1c4`, which needs a
person holding a button on hardware. No amount of work here substitutes for it.
