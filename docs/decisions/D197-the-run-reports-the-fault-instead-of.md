# D197 - The run reports the fault, instead of leaving it in the trace


**decided** · 2026-08-24

A run that ended in a guest fault printed one line: a region and an offset. Everything else
about the fault - the operation, the faulting address, the register file, the frames, the
last calls before it - was **already in the trace on disk** and was read out of it by hand,
run after run, with throwaway scripts.

That is the tool asking a person to do its job. Worse, it asks in a way that only somebody
who knows the trace schema can answer, which quietly makes the loop unavailable to anyone
else - and "someone with no knowledge can still contribute" is the point of the project.

The report now prints, on every faulting run:

```text
  faulted  write to 0xfffe0
           rax=0x0 rcx=0xfffe0 rdx=0x100000 rdi=0x4000019e9ca0
           called from 0x400000f5778a <- 0x400000000064 <- 0x4000000000a2
```

Four registers rather than sixteen: `rax`, `rcx`, `rdx`, `rdi` carry an address or a size in
almost every fault worth reading, and a full dump is a wall of hex that gets skipped.

**What this is not.** It is not diagnosis. The report describes the fault and stops; the
sentence that begins "so the allocator must have returned null" is a story, and forming it
is the reader's job. See D198 for where the line sits.

