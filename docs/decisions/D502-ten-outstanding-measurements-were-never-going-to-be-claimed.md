# D502 - Ten outstanding measurements were never going to be claimed

**decided** - 2026-09-03

`OUTSTANDING` is documented as a work queue: *"every entry in it should one day move up into a
test"*, and `OPAQUE` exists for the ones that never will, because *"keeping them there would
leave the queue with permanent residents - at which point it stops being read as a queue."*

Ten entries were in the wrong one.

```text
OUTSTANDING 95 -> 85        OPAQUE 39 -> 49
```

## The list's own example was in the queue

`OPAQUE`'s doc comment names the case that forced the table to exist:

> A module handle is the case that forced it. The console answered `0x15` and `0x14` for two
> `/app0` modules, and both runs agree, but the number reflects how many modules *that* loader
> had already placed.

`106-encoder/module-handle:loaded:handle` was sitting in `OUTSTANDING`. So was
`106-encoder/module-list:total:count`, which is the same fact stated as a total.

## What the ten have in common

None of them is a property of a call. Sorted by why:

- **The check's own input, read back.** `120-measure/identify-clocks:sceKernelUsleep:requested`
  records the sleep the probe *asked for*. A platform that ignores the request entirely
  produces the same reading. This is D497's family one layer up - there it was an
  out-parameter the probe had initialised, here it is an argument the probe chose.
- **A moment on one machine.** The three `clocks-advance` absolutes are readings taken at an
  instant, and orbistoun answers from the host clock; it cannot reproduce the console's epoch
  and nothing should make it try.
- **A rate on one machine.** The three `timer-ratio` deltas. The frequency they calibrate to
  *is* claimable and is asserted separately, which is the useful half kept.
- **That machine's configuration.** `kern.osrelease`'s length is the length of
  `0.0-prototype`, a per-machine setting, empty by default.
- **That loader's bookkeeping.** The two module-handle entries above.

## Why this is worth doing rather than tidy

A queue with permanent residents stops being read, and the count is quoted. **"95 outstanding
hardware measurements" was the headline for a day's planning**, and eleven per cent of it could
never have been worked. Moving them does not make the project more capable; it makes the number
mean what people read it as.

The measurements are still recorded and their reasons are longer than they were - what changed
is which list they are in, and therefore whether anyone will keep picking them up and putting
them down.

## What stays outstanding, and it is not obvious

`120-measure/timer-ratio:tsc_hz_calibrated:hz` stays, even though it sits among the deltas that
moved: it is a *frequency*, orbistoun already claims the reported one, and the two agree to
eight figures - so it is a claim that could be made rather than a fact about one machine.

`110-modules/tier-probe` stays too. "orbistoun models no tiers" is a scope decision, not an
impossibility, and the moment it models them the reading is claimable. That is exactly what
`OUTSTANDING` is for.
