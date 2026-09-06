# D533 - libm splits in two, and only half of it belongs in a differential

**measured** - 2026-09-04 (twenty-eight cases against glibc 2.39)

```text
differential   345  ->  373 cases
```

## The census was incomplete, and the filter was mine

D532 asked which implemented `libc` functions have no differential case, and filtered the answer
to string- and number-shaped names. That filter hid **around fifty math functions**, every one
implemented and none ever compared - a negative from a filter being a fact about the filter
(check 4), applied to my own census a tick after writing it up.

The full list is 437 uncovered. Most is correctly out of scope - pthreads, sockets, files, the
C++ ABI - because none of it is a pure function comparable by value. libm is the part that is.

## And libm splits in two

The split is the finding:

- **Exactly specified.** `sqrt`, `fabs`, `ceil`, `floor`, `trunc`, `round`. IEEE-754 and ISO C
  give each input **one** correct answer, so a bit-for-bit comparison states the contract.
- **Not specified to the last place.** `sin`, `cos`, `exp`, `log`, `pow`, `atan2`, `hypotf` and
  around forty more. Implementations are *permitted* to differ in the final ulp, so comparing
  them bit-for-bit would pin orbistoun to **glibc's libm** rather than to any contract.

That second half is the same trap as `rand` (glibc's generator) and `strerror` (glibc's wording)
- and it is a bigger one, because forty agreeing cases would look like forty verified functions
while actually asserting an implementation detail nobody promised.

## Bit patterns, because a decimal hides the case that matters

The reference records the input and the answer as raw bit patterns, which is what `strtod`
already does here. A decimal rendering hides last-place differences - and it hides the **sign of
zero**.

That is not a detail. `round(-0.5)` is `-0.0` by the round-half-away-from-zero rule, and an
implementation answering `+0.0` passes every comparison that goes through a string.

## The break, and the case that stayed quiet

`round` is half-away-from-zero. The hardware's default is half-to-**even**, so a shim built on a
rint-style instruction is wrong on exactly the halfway values. Breaking it that way:

```text
round/half-up:                returned 0x0,                 glibc 0x3ff0000000000000
round/half-away-from-zero:    returned 0x8000000000000000,  glibc 0xbff0000000000000
round/two-and-a-half:         returned 0x4000000000000000,  glibc 0x4008000000000000
round/minus-two-and-a-half:   returned 0xc000000000000000,  glibc 0xc008000000000000
```

Four cases, and the second one is `-0.0` against `-1.0` - the sign-of-zero point, appearing on
its own.

**`round/one-and-a-half` did not fire**, because half-to-even also answers 2 for 1.5. That is
the part worth keeping: the halfway cases *discriminate* rather than failing as a block, which
is what says they are testing the rule and not merely the neighbourhood.

## What this cannot prove

That the console's libm agrees with glibc on these six. It cannot - the claim is that both
implement the specified answer, and for these there is one. Where the specification permits a
range, no differential can settle it, and this does not pretend to.
