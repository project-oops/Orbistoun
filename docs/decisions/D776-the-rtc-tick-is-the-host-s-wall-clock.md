# D776 - The RTC tick is the host's wall clock in microseconds since 0001-01-01

**Status:** decided
**Date:** 2026-10-08
**known_by:** assumed (the platform's published RTC interface: a tick is a microsecond from year one)

`sceRtcGetCurrentTick` writes the host's wall clock as a tick: microseconds since
0001-01-01 00:00 UTC in the proleptic Gregorian calendar. That is the Unix time plus
719,162 days, scaled to microseconds. The clock is `orbistoun_hle::clocks::wall_clock`, the one
`gettimeofday` answers, so the two agree to the microsecond.

**Why:** PPSA03416 polls a condition with `scePthreadCondTimedwait`, reading the tick around every
wait. Both were unimplemented, so each wait answered at once and the tick never moved: a million
turns of the loop spent the run's whole call budget. A tick from a clock of its own would let
the guest's two views of the time disagree. A logical clock would not move while the guest waits
on a condition.
