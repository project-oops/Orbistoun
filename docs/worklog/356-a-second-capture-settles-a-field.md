# 2026-09-03 - (/loop) A second capture settles the third query field

```
measurements   224 -> 227      constant   200 -> 172
CLAIMED         73 -> 76       OPAQUE      58 -> 28      OUTSTANDING  69 -> 68
tests               1995
```

Arrived by hand again - the twelfth wakeup that did not fire - and the loop prompt predates the
capture this tick is about.

A new capture, `live_launch.txt`: same `meta`, same `build|dev|module`, same context as the one
before it, and a better run - **144 checks passed against 122**, three fewer failures, thirteen
fewer partials.

## The third field of the query structure is the memory type

`orbistoun-kernel` has carried this beside that field: *"What `3` denotes - a type, or some
state - is still open, and one run distinguishes them: allocate with several types and query
each back."*

That run happened. A 16 KiB page with each of `WB_ONION` (0), `WC_GARLIC` (3) and `WB_GARLIC`
(10), field read back: **`0x0`, `0x3`, `0xa`** - the type asked for, every time. Three distinct
answers also rule out the alternative obSCEne's comment named, that the field is state and the
type lives somewhere else.

**orbistoun's model was right before it could be checked.** Three claims, asserted through
allocate-then-query rather than against a region orbistoun built itself, because the latter
would pass on the model alone. Watched failing by writing a constant `1` into the field.

Only measurable because this run **got direct memory** - the previous capture recorded
`alloc-refused` for all three types. Both sets of records stay: one says what happens when the
allocation is refused, the other what the field holds when it is not.

## The poison answered D497, and the answer had to move house

Yesterday's obSCEne change poisoned that probe's out-parameter with `0xC7C7C7C7`. All
twenty-four paths now report **`0xc7c7c7c7`**: **the platform does not write it on a refused
load.**

But the id cannot carry the finding. The two captures disagree - `0x0` then `0xc7c7c7c7` - so
the generator marks all twenty-four **not constant**, and a varying measurement must never be
asserted. It is right to, even though the disagreement is between two *probes* rather than two
consoles. So the twenty-four left `OPAQUE`, and the result is recorded where a
non-constant-but-informative one belongs: an **edge case** on `sceKernelLoadStartModule`.

**I first recorded it as `known_by = measured` and a guard refused it**:
*"measured means hardware answered it. An entry claiming that and listing an open question is
contradicting itself."* The entry has open questions - the whole parked module thread - so one
measured fact does not make the entry measured. `known_by` stays `guest-observed`, matching
`sceKernelGetModuleList`/`GetModuleInfo`/`Dlsym`, which have the same shape. `known_by`
describes the entry; an edge case describes one behaviour; the second must not be promoted to
the first.

## Six of D502's ten confirmed themselves

D502 moved ten measurements out of the queue by argument - *"a moment on one machine"*, *"a rate
on one machine"*. The second capture disagrees with the first on exactly six of them, so the
generator marks them non-constant with no help from me.

**And it corrected the one I hedged on.** `tsc_hz_calibrated:hz` was kept outstanding as
"claimable in principle - agrees with the reported frequency to eight figures". Across the two
captures: `0x3d554b40` and `0x5f2476c6`, **1.03 GHz against 1.60 GHz - a 55% swing**.
`nothing_claims_a_measurement_that_cannot_be_claimed` refused it by name.

## What absorbing a capture actually costs

`cargo run -p orbistoun-gen -- measurements --records <dir>` rebuilds the table from every
capture in a directory and marks a measurement constant only when every run that took it agreed.
One capture then produces a precise work list from two gates - newly measured and unaccounted
for, and named in a list but no longer constant. Both fired, both named exact ids, and the work
was following them.

Worth writing down because the alternative is reading 11,002 lines by eye, which is how a
finding like this one stays open for another month.

## State

`cargo test --workspace` green - 119 suites, **1995 tests**, 0 failures. clippy `--tests` clean,
fmt clean, identity scan clean on both.

Nothing committed. The day holds worklogs 292-356 and D466-D507.

**Next**: the differential's uncovered functions (`strtok_r`, `strtof`, `sprintf`, `vsnprintf`),
and `035-libc/fpu-environment` - the last singleton flagged as possibly claimable.
