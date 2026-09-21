# 730. The fault report learns to read heap objects, and the null container is a zeroed allocation

**2026-09-20** — resuming worklog 725's open thread (find where the null container comes from), the trace
needed to see the faulting object's fields and could not: the fault report dumped some register targets
but not the one that mattered. Fixing that gap took one change and immediately answered the question the
trace had been circling.

## The gap: heap objects were invisible in the fault report

`describe_pointees` dumps what each register points at, but gated on `orbistoun_thunk::is_mapped` - the
title's **published** ranges, the fixed guest regions at `0x400000000000`+. A guest's own C++ objects do
not live there; they live in whatever `orbistoun-libc`'s allocator handed out, which is host heap at a
low address (`0x1d3d475afe0` this run). `is_mapped` says no to those, so the object register printed as a
bare number while `rax`, `rsi`, `rbp` and `rsp` - which happened to fall in published ranges - printed
their bytes. The one register the whole trace was about was the one left blank.

The fix: when a register is not in a published range, fall back to `read_window` - the same
VirtualQuery-checked, page-clamped read `ORBISTOUN_PEEK` already uses (worklog 724). A real heap address
reads back its bytes; a scalar (`rdx=0xa8`, `rdi=0x1a3`) reads back empty and is still dropped, so the
lines that point at objects are not buried under "not an address" noise. Nothing else changed.

## What it showed, first run

```
rbx -> 0x1d3d475afe0 = 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
r15 -> 0x1d3d475afe0 = 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
```

The object PPSA02664's `0x42d90` loop reads (rbx, held twice, in r15 too) is a **zeroed allocation**. Its
first sixteen bytes are zero, which includes the container field at `+8` that worklog 725 traced the null
`r13` to. The report now also names two null bases where it named one - `r13` and `r14`, both zero, both
fields of this same object. So the null container is not a pointer that got corrupted; it is a field of
an object that was allocated and **never populated** - the whole object is still the zero its allocation
left it.

## Why this moves the trace

Worklog 725 ended at "find where the object's container field is written". This narrows it sharply: the
field is not *written wrong*, it is *not written at all*, along with the rest of the object. So the
question is no longer "what corrupted the container" but "what was supposed to fill this object after it
was allocated, and did not run - or ran as a no-op". That points back at the AGC workload build: the
object is `0xa8` bytes, the size the phantom GetSize (worklog 729) reports, so this is the workload
buffer, allocated and left blank. The next step is to catch the allocation and see what, if anything,
writes into it before `0x42d90` reads it - which the register dump now makes a reading rather than a
guess, for this fault and every future one that faults through a heap object.

## Honest scope

This is a tool change and an observation, not a wall move: PPSA02664 still faults at the same `read of
0xa8`. But it converts worklog 725's open question from "trace an unknown pointer up an unknown number of
levels" into a concrete one - "the workload object is born zero and stays zero" - and it removes a blind
spot that would have hidden the same thing in any heap-object fault. The register dump earning its keep
on the first run it was needed is the argument for it.

## Gate state

One file changed: `crates/orbistoun-worker/src/report.rs` (`describe_pointees` falls back to the
VirtualQuery read for addresses off the published ranges). No behaviour the guest sees changes - it is a
report-time dump. `./bin/orbistoun check` green; identity scan clean. No commit.
