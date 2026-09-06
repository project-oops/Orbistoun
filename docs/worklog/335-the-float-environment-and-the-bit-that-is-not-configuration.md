# 2026-09-03 - (/loop) The float environment, and the bit that is not configuration

```
tests   1979  ->  1980
hardware claims  41  ->  45
```

The native-run capture measured the console entering a title with `MXCSR` at `0x9fe0`.
Orbistoun entered the guest with whatever the host thread carried, which has
denormals-are-zero and flush-to-zero **clear**.

That difference fails nothing. A denormal argument reads as itself here and as zero there; a
denormal result is kept here and flushed there. It surfaces as arithmetic that is quietly
different, which is the worst kind to find late - so it was worth doing, and doing it turned up
something better than the fix.

## The measured value was not the value to install (D486)

```text
0x9fe0 = 1001 1111 1110 0000
  bit 15     flush-to-zero        set
  bits 13-14 rounding             to nearest
  bits 7-12  exception masks      all six
  bit 6      denormals-are-zero   set
  bits 0-5   exception flags      0x20  <- the precision flag
```

The first four fields are what the platform **chose**. The last is what has **happened**: bits
0-5 are sticky status, and the console had done enough float work before handing over control
to set the precision flag.

**Installing `0x9fe0` would tell the guest an inexact result had already occurred** before it
executed an instruction - a fact about the console's startup reported as a fact about the
guest's arithmetic. So orbistoun installs `0x9fc0`.

The general shape, which is why it is a decision and not a comment: a measured register can mix
*how the machine is set up* with *what has happened to it*, and only the first is a property to
reproduce. Same family as D485's per-boot calibration - an artefact of when the reading was
taken rather than a claim about the platform.

## Asserted by reading the register back

Four measurements moved into `CLAIMED`. The test installs the environment and **reads `MXCSR`
back**, rather than comparing two constants - a constant comparison passes whether or not the
install happened.

It also asserts the status bits are clear, which is the half that would otherwise go unchecked.
Both failure modes were watched:

```text
the raw value copied verbatim   a status flag was installed as though it were configuration
                                left: 32   right: 0
DAZ and FTZ dropped             denormals-are-zero
                                left: 0    right: 1
```

`035-libc/fpu-environment:mxcsr:raw` stays outstanding with the decomposition as its reason. It
is not a gap - it is a measurement whose low bits are not a claim about the platform.

## Per thread, in both places that enter guest code

`MXCSR` is per-thread and a fresh host thread gets the host default, so the install happens at
**the process entry** in the worker and in **each guest thread** in `orbistoun-kernel`. Finding
the second was the point of looking: `thread.rs` enters guest code through
`enter_guest_with_argument`, so a spawned guest thread would otherwise have done denormal
arithmetic differently from the thread that spawned it.

It applies to orbistoun's own implementations answering on those threads too, which is right
rather than a side effect - they stand in for the console's C library, which runs under exactly
this environment.

## State

`cargo test --workspace` green - **117 suites, 1980 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean. Hardware: **45 claimed**, 143 outstanding, 15 opaque of
200 constants.

Nothing committed. The day holds worklogs 292-335 and D466-D486.

**Next**: the relocation pass - each title module relocated against a resolver mapping its
symbol *i* to slot `offset(M) + i`, in D482's order. `install_data_symbols` is the remaining
`OnceLock` on that path and needs merging the way the policy plants now are (worklog 334).
