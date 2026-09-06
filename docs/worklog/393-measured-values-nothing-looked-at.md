# 2026-09-04 - (/loop) Forty-seven measured values that no test looked at

```
14 measured refusal codes, none asserted before, all fourteen agree
suites 129   clippy/fmt/identity clean on both repos
```

Thirtieth cron tick. Plan item (b): the knowledge entries carry `Measured on hardware:` edge
cases naming a value - **does the code return it?**

## A census that was a fact about its own filter

47 obSCEne checks are cited by knowledge entries; `hardware.toml` holds 31; **37 of the 47 are
absent from it.** For ten minutes that looked like a hole in the gate that exists to stop exactly
that.

It is the design, and the module implementing it says so. `OBS|measure|` records carry their own
condition, so they can be asserted and become the table. The pass/fail checks put the value's
meaning in the check's C source - *"`sceKernelWrite` answering `0xffffffff80020009` to a bad
descriptor is a fact about the function, and `sceKernelGetProcessTime` answering `0xc3` is the
time it happened to be. Both arrive in the same field."* So they are quoted, never made
assertable.

Two lists that were never meant to be the same list. Recorded as a **non-finding** rather than
dropped, because the next person to notice 37 missing ids will reach the same wrong conclusion.

## What is real underneath it

Where the check id **names a refusal** the meaning is not ambiguous. Fifteen such conditions, and
nothing asserted any of them.

All fourteen checkable ones agree - fourteen separate facts, not one, because the codes are
per-subsystem and unrelated: `0x8026xxxx` audio, `0x8092xxxx` input, `0x8029xxxx` video,
`0x8002xxxx` kernel. `tests/measured_refusals.rs` holds them in two tests, **parsing the expected
value out of the knowledge base at run time** rather than copying it - a copied constant drifts from its
capture and then passes for the wrong reason (D538, D540).

## The case that shows the limit

`060-module/load-rejects-missing` depends on *which* bogus path. Outside `libkernel`, the three
firmware directories and `/app0/`: the measured ENOENT. **Under `/app0/`: a fresh handle**, since
that is where a title's own modules live. My first probe used `/app0/does-not-exist` and reported
a divergence that is not one - the branch is deliberate, and the load is recorded as having
started nothing (D514).

Which branch obSCEne's check landed in is not knowable from inside this repository, so it is
written into the test. Same caveat for all fourteen: a match says orbistoun answers the measured
code when refusing for the stated reason, not that it got the same argument.

## The break

Twice, in two subsystems, because a sweep of fourteen could be reading one constant fourteen
times. Video's `INVALID_HANDLE` moved by one fails `sceVideoOutClose`; audio's error base fails
`sceAudioOutClose`. Different cases, different files.

Decision: [D544](../decisions/D544-forty-seven-measured-values-that-no-test-looked-at.md).
