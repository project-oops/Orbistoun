# 2026-09-04 - (/loop) Two rounding rules that pin each other

```
differential   373  ->  400 cases
suites 125   tests 2013   clippy/fmt/identity clean
```

Twentieth cron tick. The single-precision half of D533's split, and one thing it does that the
double half could not.

## The pair

`roundf` is half-away-from-zero; `nearbyintf` is half-to-**even**. Both exactly specified, and
they **disagree at exactly the halfway values** - so having both means neither can be
implemented as the other without a case saying so.

That is stronger than testing either alone. D533 had to *break* `round` to demonstrate the rule,
and the demonstration went away when the break was reverted. Here it is permanent, because the
two functions are each other's control.

Breaking `nearbyintf` to half-away-from-zero fires on:

```text
nearbyintf/half    nearbyintf/negative-half    nearbyintf/two-and-a-half
```

Three cases. **Not `nearbyintf/one-and-a-half`** - both rules answer 2 for 1.5 - and **not one
`roundf` case**.

## NaN only where the answer is specified

A NaN payload is not fixed by the standard, so `sqrtf(NaN)` is out for the same reason the
transcendentals are (D533): it would test which quiet NaN this glibc produces. `fabsf` clears
the sign bit and the payload survives, so its answer *is* specified - that is the only NaN case,
and the reason sits with it.

## The bits cross identically at both widths

An `f32` argument is the low half of the float register and the answer returns zero-extended, so
the same replay serves both precisions: the dispatch gained seven names and no code. Worth
recording because it is why this was cheap - the shape was already right.

Decision: [D534](../decisions/D534-two-rounding-rules-that-pin-each-other.md).
