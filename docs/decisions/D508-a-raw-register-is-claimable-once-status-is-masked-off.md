# D508 - A raw register is claimable once status is masked off

**decided** - 2026-09-03

`035-libc/fpu-environment:mxcsr:raw` sat outstanding with a good reason:

> the raw value carries a status bit, and status is not configuration. `0x9fe0` is the four
> configuration fields - all claimed above - plus bit 5, the sticky precision flag, set by float
> work the console did before the title got control.

Orbistoun installs `0x9fc0` and deliberately does not reproduce that bit, because writing it
would tell a guest an inexact result had occurred before it executed an instruction (D486). So
the two values differ, and the raw measurement looked unclaimable.

It is not. **The split between configuration and status is published**, so the whole value is
claimable against the one thing it says about the platform:

```text
0x9fe0 & !0x3f == 0x9fc0 == GUEST_MXCSR
```

Bits 0-5 of `MXCSR` are the six exception *flags* - IE, DE, ZE, OE, UE, PE - sticky state a
program accumulates and clears by writing the register. Every other bit is configuration. That
is Intel's own definition of the register, not a division invented here to make a number match.

## The mask is the claim, not the number

The test asserts the *equality*, so it survives the console's configuration ever being measured
differently: change the measured raw value and the assertion still says "orbistoun installs the
console's configuration". A test written as `assert_eq!(live, 0x9fc0)` would pin a constant
twice and pass even if the measurement moved out from under it - the failure D486 already
recorded for this same register.

## Masking hides the direction of a difference, so the second assertion is not optional

`raw & !STATUS == live` also passes if orbistoun set a configuration bit the console clears,
because the mask is applied to only one side of the comparison in the reader's head. The test
therefore also asserts `raw & STATUS == 0x20`: the only bit the console carries and orbistoun
does not is the precision flag, exactly as D486 described it.

Watched failing by moving `GUEST_MXCSR` to `0x9f80` - a configuration bit, not a status one -
which is the break a single masked assertion is weakest against.

## The general form

A measurement that mixes configuration with accumulated state is not unclaimable; it is
claimable **against a published partition of its bits**. What makes it honest is that the
partition comes from the register's specification rather than from the difference between the
two numbers - the second would be fitting the mask to the answer, which is how a value gets
"explained" without being understood.
