# D485 - A measured value is not automatically a constant, and the counter proves it

**measured** - 2026-09-03 (a third capture falsified a standing claim)

A third hardware capture arrived and the measurement table grew from 38 entries to **224, of
which 200 are constant**. Regenerating it turned three tests red immediately, which is the
gate doing exactly what it was built for (D478). One of the three is worth a decision.

## The counter frequency is calibrated per boot

`the_counter_frequency_matches_the_console` asserted equality against one number:

```rust
assert_eq!(call("sceKernelGetTscFrequency", ..), expected);
```

It failed. The runs measured:

| runs | value | |
|---|---|---|
| `ps5-full`, `ps5-imports` | `0x5f259b8e` | 1,596,300,174 Hz |
| `console-native-run` | `0x5f259bb6` | 1,596,300,214 Hz |

**Forty hertz apart in one and a half gigahertz** - 2.5 parts in a hundred million.

The tempting reading is jitter in the measurement. It is not: **the two earlier runs agree
with each other exactly**, to the hertz. A noisy reading does not repeat. What repeats within
a session and moves between sessions is a *calibration*, so the console works out its counter
frequency at boot and reports whatever it derived.

The check's own twin agrees: `120-measure/timer-ratio:tsc_hz_calibrated` derives a frequency
from observed deltas rather than asking for one, and lands on the same figure to eight
significant digits.

## So the claim was the wrong shape, not the wrong number

Nothing may assert the value - the fold marks it `constant = false` and
`nothing_claims_a_measurement_that_cannot_be_claimed` now refuses to let it be listed at all,
not even as outstanding. A varying value is not a work item; there is no state in which
orbistoun "gets it right".

But **"orbistoun answers a frequency no console ever reported" is still a defect**, and
dropping the test would stop catching it. So the assertion became membership:

- orbistoun's answer must be one of the values a run actually measured - which forbids an
  invented number while permitting the per-boot variation;
- and the two names must agree, in every run and in orbistoun. That part *is* invariant: the
  timestamp counter and the process-time counter were measured separately and answered
  identically in all three runs.

`Measurement::values()` was added for this: the observation plus whatever the other runs saw.
**The generalisation is the point** - every non-constant measurement that is nonetheless real
can be asserted this way, and there are twenty-four of them.

## The same shape, one layer over: two reasoned entries became measured ones

`OPAQUE` held this, written from reasoning rather than evidence:

> `app0-libc` - 0x15 is the console loader's own handle numbering, not a property of the call

The third capture answered `0x14`. **The reasoning was right and is now measured**, and the
entry is gone - not because it was wrong but because the fold now marks it non-constant
automatically. A hand-written classification was replaced by a mechanical one, which is
strictly better: the next capture updates it without anybody re-reading a comment.

Same for `app0-fios2`, which went further and answered `0x80020063` where it had answered a
handle. Four entries left the lists this way.

## What this says about the vocabulary

`measured` has always meant "a run observed this". It does **not** mean "the platform always
does this", and the two are separated by the `constant` field rather than by judgement about
what a `hz` or a `handle` ought to be (D478). This is the first case where the distinction
changed a test rather than merely a label, and the mechanism found it without being asked.

**One capture cannot tell a constant from a calibration.** Two agreeing captures cannot
either - these two did, for weeks. Only disagreement is informative, so the value of the third
run is not the 266 measurements it added but the two claims it took away.
