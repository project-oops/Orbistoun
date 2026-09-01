# D365 - The handoff structure answers by being asked


**decided** - 2026-08-29

The payload wall had been "the entry point wants a structure nobody here can describe" for
three sessions, with two routes left and both expensive. It took three runs.

### The three blocks, and the question each one answers

`Sentinels` fills every field with a distinct unmapped marker, so a fault **names the field**
- one field per run, and the run ends there. `Answering` points every field at a function
that returns zero, so the guest **gets further** - but it answers zero to everything and says
nothing about what was asked.

Neither can answer *what was passed*. So a third: every field points at its own emitted stub
that shifts the guest's arguments along by one register, puts its own slot index in the
first, and tail-calls a reporter.

```text
mov rcx, rdx     the guest's third argument moves to the fourth
mov rdx, rsi     its second to the third
mov rsi, rdi     its first to the second
mov rdi, imm64   the slot index takes the first
mov r11, imm64   the reporter
jmp r11
```

### What it said, immediately

```text
the guest called handoff slot 0 with (0x1, 0x4000000109f7, 0x600000800ef0)
```

The second argument is inside the payload's own `.rodata`. At that offset, in its own file,
is the string **`sceKernelDlsym`**. The third is a stack address.

So field zero is a name resolver, and the payload's very first act is to resolve the resolver
- bootstrapping it before anything else. What made this cheap is that the guest supplied the
evidence itself: the string is in its file, at an offset its own registers named, so nothing
was inferred and nothing was read out of anybody's firmware.

### And the fields it does not know are better mapped than absent

Markers are unmapped, so any use of one ends the run. That is right when the question is
"which field" and wrong once the answer is being used: a runtime reading six fields would
need six runs. Mapping the marker range read-write means a field read as a pointer yields
zero - which a correct program checks - while the address still says which field it was. The
guest got two walls further in one run.

### The earlier correction, corrected back

An earlier session claimed `sceKernelDlsym` was the item deciding whether payload support was
native or a hack, then withdrew it on measurement: no payload imports it. The measurement was
right and the withdrawal went too far. It is not imported **because it arrives through the
handoff structure**, which is a stronger position than being imported, not a weaker one.

