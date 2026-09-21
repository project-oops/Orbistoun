# 732. The flagged AGC-driver setters: verified unimplementable as 0x0, held for a3f0

**2026-09-20** — with PPSA02664's real wall blocked on obSCEne `a3f0` (the `sceAgcInit` state-buffer
measurement, worklog 731), this tick tested the only tangential thing the run report keeps flagging: the
three unimplemented `libSceAgcDriver` calls. It ends net-zero on purpose - the value is confirming they
must *not* be implemented the obvious way, before a future session writes the regression.

## What was tried, and why it was reverted

The report ranks `sceAgcDriverSetTFRing` and `sceAgcDriverSetHsOffchipParam` as gaps to implement. They
are non-exports (obSCEne c9e2), so the title inlines them, and the tempting synthesis is "a setter
returns success, answer `0x0`". They configure the tessellation-factor ring and the hull-shader off-chip
buffer, which this title never uses, so `0x0`-and-do-nothing looked honest.

Implemented that way and run, PPSA02664 went **BACK** - reaching less of the interface than the
placeholder record does. So the change was reverted before it left the tick. The `sceAgcDriverSetTFRing`
knowledge already carried the reason (worklog 722): Unity consumes the return, a guessed one is wrong
(D708), and the real one is unmeasurable. The placeholder these currently answer is what gets furthest,
because the guest reads it as "not this path" and routes around it, where a `0x0` sends it down a path it
cannot complete.

## The subtlety worth recording

Forcing each setter to `0x0` **individually** (`ORBISTOUN_RETURN`) reads as verdict `same`, not BACK -
only *both* together regressed. That is not a contradiction so much as a warning about the measure: the
reach verdict is relative to a moving best-ever record, so a single-lever test taken after a
higher-reaching run scores `same` against a record it did not lower. The reliable signal is the
whole-implementation run, and that one went BACK. So the conclusion is the implementation's, not the
isolated probe's: these stay unimplemented, answering the placeholder, until their returns can be
measured - which, being non-exports, they cannot be from an export table, only synthesised once the
context they operate in is understood.

`sceAgcDriverAddEqEvent` was left out of even the attempt: it registers an event source, so a bare `0x0`
would be a false success in a way a pure setter's is not, and it belongs with the event-queue synthesis,
not here.

## Where this leaves the title

Unchanged and blocked, honestly. PPSA02664's one wall is the zeroed workload downstream of an unfilled
`sceAgcInit` context (worklog 730-731), and that is waiting on `a3f0`. The flagged driver setters are
now confirmed not to be a cheaper way around it - the report flags them because they are unimplemented,
not because implementing them helps, and this tick is the check that separates those two.

## Gate state

No file changed under `crates/` - the `orbistoun-gpu/src/agc_driver.rs` edit was made, measured, and
reverted to net-zero. `./bin/orbistoun check` green; identity scan clean. No commit.
