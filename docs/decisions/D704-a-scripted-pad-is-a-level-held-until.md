# D704 - A scripted pad is a level held until the next step, and carries typed state rather than bytes

**Status:** decided
**Date:** 2026-09-16

## The choice

`crates/orbistoun-input/src/script.rs` defines the deterministic input route
`REQ-20260915T0929Z-02a0` asks for: a file of timed pad states, so a run that has to get past a
licence page or a press-to-start is repeatable. Four things were decided in writing it.

**A step sets the pad and it holds until the next one.** Not an event queue. This follows
`latest.rs`, which keeps the most recent state per port for the reason recorded there: a title
asks what the pad is doing *now*, and replaying a backlog of finished presses is worse than
nothing. So a press and its release are two steps and the gap between them is the hold; a press
with no release is held to the end of the run, which is a real thing to want and therefore not an
error.

**Steps out of order are refused, not sorted.** Including two at the same instant, which is
ambiguous rather than merely misordered. Sorting would run something other than what the file
says while looking like it worked.

**The script carries `PadState` - typed buttons, sticks, triggers - and never bytes.** Which
offset inside the measured 120-byte record carries the buttons is *not settled* and cannot be
settled here: it is an inference from a single at-rest image (D345), and the measurement that
would fix it needs somebody physically holding a button (obSCEne `REQ-20260910T0650Z-d1c4`, open
since 2026-09-10 and still `pending` for exactly that reason, across every sweep since).

**Deserialising is the caller's job.** The types derive `Deserialize` and nothing in this crate
reads a file, which is the shape `mapping.rs` already set.

## Why

- **The two unknowns are separable, and separating them is the whole value.** The *mechanism* -
  what the pad does and when - is ours, testable today, and has no hardware question in it. The
  *encoding* - which byte a title reads - is a measurement nobody has taken. Mixing them would
  make the route wait on a hardware run that needs a person in a room with a controller, and
  would put inferred offsets into a file format that then could not change. Keeping them apart
  means every script written now keeps working unaltered when the encoding lands, because what
  changes then is `pad.rs`.
- **Level rather than stream is not a simplification, it is the correct model.** A pad has no
  events; it has a position, sampled. An event queue would have to invent a sampling rate and
  would drift against the guest's own polling.
- **Refusing disorder rather than sorting** is the same rule this repository applies to a guard
  that could paper over its input. A sorted script is a different script, run silently.

## What this does not do

**It is not wired into a run yet.** This is the format, the reader and the rule for what the pad
is doing at a given moment; nothing yet feeds it to a guest as time passes, and no compatibility
run uses one. That wiring belongs with the rest of `02a0` - minute-scale runs and an outcome that
tells a waiting guest from a finished one - and touches the worker, which is a separate change.

It also carries one pad. A port field would cost little, but nothing has reached a second pad and
a seam that pays off only hypothetically is speculation (principle 12); the format can gain one
without breaking a script written today, because an absent field takes its default.
