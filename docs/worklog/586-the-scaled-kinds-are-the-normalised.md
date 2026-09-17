# 586. The scaled kinds are the normalised ones with the division removed

**2026-09-15** - orbistoun-translate, continuing the packed-format work

`USCALED` and `SSCALED` translate and execute. Eighteen more format codes, and no new
arithmetic: both are a kind already built with one operation taken out.

```
x=0  y=1  z=128  w=255   ->  0.0  1.0  128.0  255.0
x=-128 y=-1 z=1 w=127    ->  -128.0  -1.0  1.0  127.0
```

## 1. Why this was the next one

The packed path grows one component kind at a time, lowest risk first, because a wrong
conversion here does not fail - it renders the wrong thing, silently. The two scaled kinds
are the lowest-risk pair there will ever be: they are `UNORM` and `SNORM` with the divide
deleted. The field extraction is the same call, the integer-to-float convert is the same
call, and what they add is nothing.

`SSCALED` also has **no clamp**, and that is a decision rather than an omission. `SNORM`
clamps because -128/127 falls a hair outside the -1.0..=1.0 the format promises. An
unscaled value promises no range, so there is nothing to clamp it to.

## 2. The two mistakes, each ruled out by a component

Both tests run on a device and assert float bits with no epsilon, and the values were
picked so that each plausible error fails on a specific component rather than drifting:

| component | value | what it rules out |
|---|---|---|
| `USCALED` z = 128 | `128.0` | a leftover divide, which would give `0.502` |
| `SSCALED` x = -128 | `-128.0` | a missing sign extension, which would give `128.0` |
| `SSCALED` x = -128 | `-128.0` | a leftover divide, which would give `-1.0` |

Every one of those wrong answers is a number a rendered frame would show as a plausible
colour. That is the whole reason for asserting bits.

**The test was made to fail before it was trusted.** Asserting `129.0` reported the device
had computed `0x43000000`, which is 128.0 exactly - so the assertion is live and the
arithmetic is the device's rather than a stub's.

## 3. Two things clippy was right about

Adding the arms made `packed_component`'s wildcard cover exactly one variant, and clippy
said so. Naming `Srgb` explicitly is better than the wildcard was: the match is exhaustive,
a kind added to the table later will not fall silently into an `unreachable!`, and the arm
now carries **why** sRGB is the odd one - its conversion is a piecewise transfer curve, not
arithmetic on the field.

The refusal check then came out into `packed_format_admitted`, which is the length lint
being right for a real reason: every kind admitted there has a lane of arithmetic below it,
and keeping the two lists in one function each is what makes the `unreachable!` a statement
about the pairing rather than a hope.

## 4. What is still refused, by name

- The **narrower packed floats** - the 11- and 10-bit channels of `10_11_11` and `11_11_10`.
  They are not IEEE halves and decode differently; nothing here has measured how.
- **`SRGB`**, for the reason above.
- **Formats wider than one word** - the whole `16_16_16_16` family, which needs a second
  word read and a bounds check over two.
- **Packed stores.**

Each names which, so a shader that needs one is a loud gap rather than a quiet wrong render.

## 5. Also: an advisory the gate found

`cargo deny` reported RUSTSEC-2026-0285 against `rustls` 0.23.43, reached through `reqwest`.
Updated to 0.23.45 and the gate is clean. Not a finding of this project's own, but the gate
is there to be acted on rather than read past.

## 6. Files

- `crates/orbistoun-translate/src/model.rs` - the two arms, the named `Srgb` arm, and
  `packed_format_admitted`.
- `crates/orbistoun-translate/tests/execute.rs` - both device tests.
- `docs/roadmap/015-phase-6-s-contents-built-ahead-of-it.md` - G10's row, which had said
  the narrow-component conversion was not begun.

## Next

1. The `16_16_16_16` family, which is the first packed format needing two words.
2. `REQ-20260914T2348Z-4e71` and `REQ-20260914T1720Z-9c4a`, both still open on the obSCEne
   bus.
