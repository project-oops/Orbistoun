# D534 - Two rounding rules that pin each other

**measured** - 2026-09-04 (twenty-seven cases against glibc 2.39)

```text
differential   373  ->  400 cases
```

The single-precision half of D533's split, and one thing it can do that the double half could
not.

## The pair

`roundf` is half-away-from-zero. `nearbyintf` is the current rounding mode, which is
half-to-**even** by default. They are both exactly specified, and they **disagree at exactly the
halfway values**.

So having both in the corpus means neither can be implemented as the other without a case saying
so. That is stronger than testing either alone: D533 had to *break* `round` to demonstrate the
rule, and the demonstration went away when the break was reverted. Here the demonstration is
permanent, because the two functions are each other's control.

Breaking `nearbyintf` to half-away-from-zero:

```text
nearbyintf/half              nearbyintf/negative-half              nearbyintf/two-and-a-half
```

Three cases. **Not `nearbyintf/one-and-a-half`** - both rules answer 2 for 1.5 - and **not one
`roundf` case**. A break that fires on everything proves less than one that fires on the cases
written for it, and this fires on three of the four halfway values and nothing else.

## NaN is here only where the answer is specified

A NaN payload is **not** fixed by the standard: `sqrtf(NaN)` may answer any quiet NaN, and
comparing bit patterns there would test which one this glibc happens to produce - the same trap
as the transcendentals, one level down.

`fabsf` is different. It clears the sign bit and the payload survives, so the answer *is*
specified. That is the only NaN case, and the reason is written where the case is.

## The bits cross identically at both widths

An `f32` argument is the low half of the float register and the answer comes back zero-extended,
so the same replay serves both precisions - the dispatch gained seven names and no code. Worth
recording because it is the reason this was cheap: the shape was already right.

## What this cannot prove

The same limit as D533. That the console's libm agrees with glibc on these is not the claim; the
claim is that both implement the *specified* answer, and for these there is one.
