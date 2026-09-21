# 725. Tracing the null container: the phantom is a GetSize, not the producer

**2026-09-20** — used `ORBISTOUN_PEEK` (worklog 724) to trace PPSA02664's null-container memcpy back toward its producer. Two things are now settled that guesswork
had wrong.

## The phantom `0x7d86501b8094ef57` is a GetSize, and it is not the direct cause

Dumping and disassembling its call site (`0x4000000435b5`) shows the shape of a size query, not an
object constructor:

```
lea  rdi, [rbp-0x470]      ; arg0 is an out-param (a stack slot)
call 0x7d86501b8094ef57
mov  rbx, [rbp-0x470]      ; read the value it wrote back
add  rbx, 7; and rbx, ~7   ; align it up to 8 - it is a byte size
... rbx used as the size for three buffer calls (0x40200/0x40220/0x40230)
```

So the phantom writes a **byte size** into its out-param. That is far more tractable than the
"complex workload object" earlier worklogs assumed - a size is a scalar. But it is **not** what nulls
the container the run dies on: planting a plausible size (`ORBISTOUN_WRITE=…:0+0:0x100`, "2 planted")
left the fault identical, exactly as forcing its return did (worklog 722). Both of the phantom's
levers - its return and its out-param - are ruled out as the memcpy's cause. It is a real gap to
implement (a size to compute), but a different one from this fault.

## Where the null container actually comes from

The faulting memcpy is inside a function at `0x400000042d90`; its prologue is
`rbx = rdi` (arg0, the object being processed) and `r13 = rsi` (arg1). The source the memcpy reads is
`r13`, derived across the loop from the object in `rbx`. So the null container is **not produced in
this function** - it arrives as one of its arguments, from the caller. The producer is one level up,
in whatever builds and passes that object.

## Disposition

This is a multi-level whole-program trace, and each level is one `ORBISTOUN_PEEK` + disassembly. Two
levels are done (the memcpy loop, and the function that owns it); the next is the caller of
`0x42d90`, to find where the object's container field is written - which is the producer the
six-non-export inline chain (worklog 723) should have run. The tool has turned each level from a
guess into a reading; the work now is walking the levels.

Separately, the phantom-as-GetSize finding is worth its own follow-up: a size is computable, and
`sceAgcDcbSetCxRegistersIndirectGetSize` (a named, obSCEne-measured sibling) is the reference for what
that size should be - a smaller, self-contained sub-problem than the container.

## Gate state

No code changed - three `ORBISTOUN_PEEK` runs, disassembly, and one `ORBISTOUN_WRITE` diagnostic
(verdict caveated, not recorded). `./bin/orbistoun check` was green as of worklog 724. Identity scan
clean.
