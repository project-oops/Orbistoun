# D486 - A measured register holds configuration and status, and only one is reproducible

**measured** - 2026-09-03 (from the native-run capture, `035-libc/fpu-environment`)

The console enters a title with `MXCSR` at **`0x9fe0`**. Guest code runs natively, so without
doing anything about it a guest uses whatever the host thread carries - which has
denormals-are-zero and flush-to-zero **clear**, because that is the ordinary host default.

A denormal argument therefore reads as itself in orbistoun and as zero on the console, and a
denormal result is kept here and flushed there. Nothing about that fails a call. It shows up as
arithmetic that is quietly different, which is the hardest kind of divergence to find later -
so it is worth fixing, and this decides what "fixing" means.

## The measured value is not the value to install

Decomposed, `0x9fe0` is:

| bits | field | value | kind |
|---|---|---|---|
| 15 | flush-to-zero | set | configuration |
| 13-14 | rounding mode | to nearest | configuration |
| 7-12 | exception masks | all six masked | configuration |
| 6 | denormals-are-zero | set | configuration |
| 0-5 | exception flags | `0x20` - precision | **status** |

The first four are what the platform *chose*. The last is what has *happened*: bits 0-5 are
sticky status, and the console had done enough float work before handing the title control to
set the precision flag.

**Installing `0x9fe0` would tell the guest an inexact result had already occurred**, before it
executed a single instruction - a fact about the console's startup, reported as a fact about
the guest's own arithmetic. So orbistoun installs `0x9fc0`: the same configuration, status
clear.

This is the general shape and not a quirk of one register. A measured register, a measured
status word, a measured flags field - each may mix "how the machine is set up" with "what has
happened to it", and **only the first half is a property to reproduce**. The second half is an
artefact of when the reading was taken, in the same family as D485's per-boot calibration.

## What is asserted, and what stays outstanding

The four configuration fields moved into `CLAIMED` and are asserted **by reading the register
back** after the install, rather than by comparing two constants - a comparison of constants
passes whether or not the install happens.

`035-libc/fpu-environment:mxcsr:raw` stays outstanding, with the decomposition as its reason.
It is not a defect and not a gap: it is a measurement whose low bits are not a claim about the
platform.

The test also asserts **the status bits are clear**, which is the half that would otherwise go
unchecked - installing `0x9fe0` verbatim passes every configuration check and is still wrong.
Watched failing both ways: the raw value copied verbatim (`left: 32, right: 0`), and DAZ/FTZ
dropped (`denormals-are-zero: left: 0, right: 1`).

## Per thread, because the register is

`MXCSR` is per-thread and a fresh host thread gets the host default, so **every path that
enters guest code installs it**: the process entry in the worker, and each guest thread in
`orbistoun-kernel`. A thread that skipped it would do denormal arithmetic differently from the
thread that spawned it, which is worse than every thread being wrong the same way.

It also applies to orbistoun's own implementations answering on those threads. That is correct
rather than a side effect: they stand in for the console's C library, which runs under exactly
this environment.
