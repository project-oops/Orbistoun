# D175 - The filesystem is exonerated, and four walls are not one wall


**decided** · 2026-08-21

Two questions asked in order after `image+0x43c4` resisted eight attempts.

### Is the data the guest loaded actually correct?

Verifying *content* would double every read - re-reading each file to compare. Verifying
**completeness** costs a counter and catches the failure that matters: a title that silently
receives a truncated asset then faults inside its own parser, which is exactly the shape of
wall that cannot be attributed.

Short reads are counted, with one distinction that makes the count mean something: reading
to the end of a file *is* a short read, and counting it would bury the case that matters. A
read cut short **before** the end is the defect, and the two are separated by whether the
file had already finished.

```text
files    10 reads, 11328 KiB, none cut short
```

**The guest receives every byte it asks for.** The filesystem is exonerated, and the fault
is downstream of it - which is worth as much as finding a bug would have been, because it
removes a whole layer from suspicion.

Printed on every run that reads anything, including when clean, for the same reason the
stack-conformance line is: a line that only appears on failure cannot be told apart from
one nobody wired up.

### The corpus says the wall was never singular

| title | result |
|---|---|
| PPSA28061 | 47 imports, 933 calls, `image+0x43c4` |
| PPSA25872 | 14 imports, **1735 calls**, `image+0x7b591e` |
| PPSA04263 | spins on `sceKernelDirectMemoryQuery`, hits the time limit |
| PPSA03416 | illegal instruction at `image+0x1595bc9`, `rax` holding our error code |
| PPSA02664 | **does not parse** - previous-generation wrapper |
| PPSA21564 | **does not parse** - previous-generation wrapper |

**Four distinct walls in four different places.** `image+0x43c4` had begun to feel like
*the* blocker, and optimising against one title is how that happens. The loop is meant to be
run across the corpus.

PPSA03416 is the more tractable lead: three frames from the entry point, `rax` holding
`0x7FFF0001`, and execution at an offset twenty-two megabytes into the image - which reads
as the guest calling through a pointer a stub handed it. Far closer to the surface than
Earthion's wall.

And **two of six titles do not parse at all**. That is the only item here that changes the
denominator rather than one numerator.

