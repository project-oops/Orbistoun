# 2026-09-04 - (/loop) libm splits in two, and only half belongs here

```
differential   345  ->  373 cases
suites 125   tests 2013   clippy/fmt/identity clean
```

Nineteenth cron tick.

## The census was incomplete, and the filter was mine

D532 asked which implemented `libc` functions have no differential case - and filtered the
answer to string- and number-shaped names. That hid **around fifty math functions**, all
implemented, none ever compared. A negative from a filter being a fact about the filter (check
4), applied to my own census one tick after writing it up.

437 uncovered in full. Most is correctly out of scope - pthreads, sockets, files, the C++ ABI -
none of it comparable by value. libm is the part that is.

## The split is the finding

- **Exactly specified**: `sqrt`, `fabs`, `ceil`, `floor`, `trunc`, `round`. One correct answer
  per input, so a bit-for-bit comparison states the contract.
- **Not specified to the last place**: `sin`, `cos`, `exp`, `log`, `pow` and ~forty more.
  Implementations may differ in the final ulp, so comparing them would pin orbistoun to
  **glibc's libm** rather than to any contract.

The second half is the `rand`/`strerror` trap again, and a bigger one: forty agreeing cases
would look like forty verified functions while asserting an implementation detail nobody
promised.

## Bit patterns, because a decimal hides the case that matters

A decimal rendering hides last-place differences and the **sign of zero**. `round(-0.5)` is
`-0.0`, and an implementation answering `+0.0` passes every comparison that goes through a
string.

## The break, and the case that stayed quiet

`round` is half-away-from-zero; the hardware default is half-to-**even**. Breaking it that way:

```text
round/half-up:              returned 0x0,                glibc 0x3ff0000000000000
round/half-away-from-zero:  returned 0x8000000000000000, glibc 0xbff0000000000000
round/two-and-a-half:       returned 0x4000000000000000, glibc 0x4008000000000000
round/minus-two-and-a-half: returned 0xc000000000000000, glibc 0xc008000000000000
```

The second is `-0.0` against `-1.0` - the sign-of-zero point on its own.

**`round/one-and-a-half` did not fire**, because half-to-even also answers 2 for 1.5. The
halfway cases *discriminate* rather than failing as a block, which is what says they test the
rule and not the neighbourhood.

## One instrument note

The build script did not link libm, so the first regeneration failed at the linker - and **the
script's own guard refused to install the short run**, leaving the committed file untouched at
345. That guard has now paid for itself. `-lm` added.

Decision: [D533](../decisions/D533-libm-splits-in-two-and-only-half-belongs-here.md).
