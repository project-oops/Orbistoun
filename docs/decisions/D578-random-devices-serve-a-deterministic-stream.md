# D578 - Random devices serve a deterministic stream

**Status:** decided
**Date:** 2026-09-07

`/dev/random` and `/dev/urandom` are one non-blocking device reading from
`orbistoun_core::entropy`, a fixed sequence shared with every other randomness source; a write
to it is refused. Recording successful opens is a setting, `ORBISTOUN_TRACE_OPENS`, off by default.

**Why:** a random device's bytes carry no meaning, so it can be served without inventing a layout
or vendor semantics, and FreeBSD makes both names one device. Every measurement here rests on two
runs behaving identically, and a physically random seed sends the guest down a different path
each run. One pool keeps two readers from drifting apart. A write stirs nothing in a fixed
sequence, so accepting it would claim an effect. Recording opens changes only what is reported,
so it carries no caveat, and it is gated because successes are frequent.

**Rejected:**
- Host randomness: two runs of one build stop being comparable.
- Leaving the devices absent: they carry no semantics that could be answered wrongly.
- Always recording opens: a lock and a string per open changes what it observes.
