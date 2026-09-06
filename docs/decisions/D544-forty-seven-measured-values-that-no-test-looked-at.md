# D544 - Forty-seven measured values that no test looked at

**decided** - 2026-09-04

Item (b) of the plan: the knowledge entries carry `Measured on hardware:` edge cases naming a
value; **does the code return it?** D542 checked those entries' labels and never their values.

## First, a census that was a fact about its own filter

Forty-seven distinct obSCEne checks are cited by knowledge entries. The measurement table
`hardware.toml` holds thirty-one. **Thirty-seven of the forty-seven are absent from it** - which
looked, for about ten minutes, like a coverage hole in the gate that exists to stop exactly that.

It is not. It is the design, and the module that implements it says so plainly. Two pipelines
read the same captures:

- **`OBS|measure|` records** carry check, subject, condition and kind, so the value's meaning is
  in the record. They become `hardware.toml`, and `tests/hardware.rs` gates every constant one
  as claimed or outstanding.
- **The pass/fail checks** put the value's meaning in the check's own C source. *"`sceKernelWrite`
  answering `0xffffffff80020009` to a bad descriptor is a fact about the function, and
  `sceKernelGetProcessTime` answering `0xc3` is the time it happened to be. Both arrive in the
  same field."* So they are quoted into knowledge entries and deliberately never made
  assertable.

I compared two lists that were never meant to be the same list - the fifth time in this run that
a census answered a question about its own filter. Worth recording as a non-finding rather than
quietly dropping, because the next person to notice 37 missing ids will reach the same wrong
conclusion.

## What is real underneath it

The caution is right in general and unnecessary for a subset. Where the **check id names a
refusal** - `close-rejects-bad-handle`, `open-rejects-null`, `mutex-unlock-unheld` - the value is
not ambiguous. It is the code the console answers when refusing, and orbistoun either answers it
or does not.

Fifteen such conditions. Nothing asserted any of them.

```text
sceAudioOutClose        0x80260003     sceKernelClose       0x80020009
scePadClose             0x80920003     sceKernelLseek       0x80020009
sceVideoOutClose        0x8029000b     sceKernelRead        0x80020009
sceVideoOutSetFlipRate  0x8029000b     sceKernelWrite       0x80020009
sceKernelPollEventFlag  0x80020003     sceKernelOpen        0x80020002 / 0x8002000e
sceKernelMunmap         0x80020016     sceKernelLoadStartModule  0x80020002
scePthreadMutexUnlock   0x80020001     sceKernelDlsym       0x80020003  (D543 covers it)
```

**All fourteen agree.** That is fourteen separate facts, not one: the codes are per-subsystem and
unrelated - `0x8026xxxx` audio, `0x8092xxxx` input, `0x8029xxxx` video, `0x8002xxxx` kernel - and
each is a value a guest can switch on. `tests/measured_refusals.rs` now holds them, in two
tests - the eight that need only a bad argument, and the six that need a buffer, a path or an
object built first.

**The expected values are parsed out of the knowledge base at run time**, not copied into the
test. A copied constant drifts from the capture it came from and then passes for the wrong
reason, which is where D538 and D540 both ended up. A re-absorbed capture that changes a code
makes this file ask about the new one with nobody editing it.

## The case that shows the limit

`060-module/load-rejects-missing` loads a bogus path, and orbistoun's answer depends on *which*:

- outside `libkernel`, the three firmware directories and `/app0/` - the measured ENOENT;
- **a nonexistent path under `/app0/` - a fresh handle**, because that is where a title's own
  modules live and the loader has already placed them.

My first probe used `/app0/does-not-exist` and reported a divergence that is not one. The branch
is deliberate and documented, and the load is recorded as having started nothing (D514) - the
mitigation principle 3 asks for. But it means this test's agreement covers the branch it
exercises and not the other one, and **which branch obSCEne's check landed in is not knowable
from inside this repository**. Written into the test rather than left as a comfortable pass.

That is the general caveat for all fourteen: the check id names the *condition*, so a match says
orbistoun answers the measured code when refusing for the stated reason - not that it was given
the same handle, descriptor or path.

## The break

Made to fail twice, in two subsystems, because a sweep of fourteen cases could be reading one
constant fourteen times. Moving video's `INVALID_HANDLE` by one fails on `sceVideoOutClose`;
moving audio's error base fails on `sceAudioOutClose`. Different cases, different files.
