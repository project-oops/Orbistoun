# 2026-08-29 - The payload wall, answered by asking the guest


The wall had stood for three sessions: the payloads' runtime rejects a handoff structure
nothing here could describe, and the two remaining routes were both expensive. It took three
runs and a stub that shifts four registers.

### `vsnprintf` first, because it is the most wanted name there is

Twenty-two of the twenty-five payloads import it - ahead of every other missing name, and
ahead of `write`, `sleep` and the whole socket set. Every logging helper they have is built
out of it.

A `va_list` on this architecture is not a pointer walking a stack: it is a cursor over the
*caller's own* spilled registers and the stack past them, four fields, psABI section 3.5.7.
Which means the `v` forms are not a convenience wrapper - they can render a format the
register forms have to refuse, because six registers is where those stop and a list has no
such limit (D364). The test asserts exactly that, both ways, on one format.

One renderer, two argument sources. Two formatters would have drifted and the first
divergence would have been a string that differed by which spelling of `printf` was called.

### And then the wall itself

Three blocks, three questions. Markers say **which** field is used and end the run.
Answering stubs say **how far** it gets and say nothing about what was asked. Neither can
say **what was passed**, so: one emitted stub per field, shifting the guest's arguments along
by a register, putting its own slot in the first, tail-calling a reporter.

```text
the guest called handoff slot 0 with (0x1, 0x4000000109f7, 0x600000800ef0)
```

That second argument is in the payload's own `.rodata`. At that offset, in its own file, is
`sceKernelDlsym` (D365).

The guest supplied the evidence. Nothing was inferred, nothing was read out of firmware, and
the whole thing cost one run.

### An import table was never going to answer it

`klogsrv` carries `vsnprintf`, `snprintf` and `sprintf` as eight-byte **objects in `.bss`**.
Its runtime resolves them one at a time and stores them there. Those names are in no import
table and no relocation - which is why entering at `main` left them null, and which settles
what D359 half-saw: the null *is* a `.bss` global, and it is null because the runtime that
fills it never ran.

So the thunk table grew a second population - one stub per implemented name, published by
name - and `sceKernelDlsym` answers out of it. **The same address the linker would have
written**, so a function reached by name and the same function reached by an import are one
address with one counter and one trace entry. A resolver minting its own answers would have
made a call behave differently depending on which route found it (D366).

### Three things that had to stay true, and one that was already false

A stub count still means the guest's imports; the label list and the binding list are now one
list with a test; and `unknown` became `unknown#index`, because two unlabelled stubs read as
one function called twice - which is exactly what they did, this session, before the change.

The one already false: `a_format_that_cannot_be_honoured_empties_the_destination` asserted a
specific value in `first_fault`, which is one slot for the whole process. It passed because
nothing else had ever recorded a fault. The first `vsnprintf` test that did made it fail, and
the test was the wrong thing rather than the new one - **fourth appearance** of the shared
state hazard, same fix: ask the thing that computed the answer.

### Where it got to

`klogsrv` runs `__crt_start` at its declared entry for the first time, resolves
`sceKernelDlsym` and `getpid`, reaches `__kernel_init`, and stops on handoff field 2 - which
it reads as a pointer. Two walls further than this morning, and the next one is a
measurement rather than a mystery: one more run of the reporting block describes it.

Unknown fields are mapped read-write now rather than left unmapped. A field read as a pointer
yields zero - which a correct program checks - while the address still names the field, so a
runtime that reads six of them does not need six runs.

### The correction, corrected back

An earlier session called `sceKernelDlsym` the item that decides whether payload support is
native or a hack, then withdrew it: no payload imports it. The measurement was right and the
withdrawal went too far. It is not imported **because it arrives through the handoff
structure** - which is a stronger position than being imported, not a weaker one.

### The second field, and three smaller things

With the resolver in place the same technique answered the next field. Two runs, differing
only in what the unestablished fields hold:

| fields | what happened |
|---|---|
| mapped markers | `__kernel_init` reads field 2, carries on, and hands `0x2001` back as a module handle |
| zero | it reads field 2 and **faults dereferencing null** |

Field 2 is a pointer the runtime reads through (D368). And `0x2001` is the marker's own low
half plus one - which is the caution worth writing down: **a marker a guest uses as data
becomes a value the guest computes with**, so it names the field and makes the next few calls
fiction. Which fill is in use is `ORBISTOUN_HANDOFF_FIELDS` now, defaulting to markers, and
`orbistoun-env` records it so two runs are never compared across it.

**Time and waiting**, because between them `sleep`, `gettimeofday`, `time`, `usleep` and
`clock_gettime` are wanted by more payloads than the whole socket set - a server's loop is
*wait, poll, timestamp, log*, and it does the waiting first. A `sleep` that returns
immediately does not save a guest any time; it turns a paced loop into a spin. All six are
POSIX-documented outright, and the two structures are in the FreeBSD checkout the constants
come from, so `_clock_id.h` joined the harvest rather than the identifiers being typed in.

**`write`**, which seventeen payloads import and which was simply the POSIX spelling of a
call this project already served.

**And a rule that had been implicit.** `clock_gettime` and `gettimeofday` are C library
functions, so they were written in `orbistoun-libc` - and the audit refused it, because both
were already declared in `libScePosix`, where a title was measured importing them. Where a
symbol is *declared* is a claim about the target; where its *code lives* is a claim about this
repository (D367). They are answerable separately, they were conflated because they usually
coincide, and the audit is what noticed.

