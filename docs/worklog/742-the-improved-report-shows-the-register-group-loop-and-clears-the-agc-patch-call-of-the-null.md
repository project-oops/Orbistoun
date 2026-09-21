# 742. The improved report shows the register-group loop, and clears the AGC patch call of the null

**2026-09-21** — first run of PPSA02664 with the copy-source finding from 741 in place. It paid off on
the first run: the fault report now lays out the **register-group build loop** that produced the wall,
and the shape it shows — nine small copies that succeed, one large copy that faults — reframes the
target and, with the measured AGC knowledge, rules a tempting fix out. No code changed this tick; this
is the observation the last two ticks' tooling was built to make.

## What the run showed that earlier runs could not

The tail, now carrying copy operands and the named copy-source lead (740, 741):

```
>> libc::memcpy faulted reading its source: given src 0xa8 n 0x50, whose base is in the null page
   (0x0 + 0xa8); the faulting address is byte 0x0 into that source
-> libc::memcpy is a faithful byte copy ... the gap is whatever produced its source pointer 0xa8 ...

just before: libc::memcpy(0x…f070) src 0xa8 n 0x50 from 0x…42ebd            <- faults
just before: libSceAgc::sceAgcSetCxRegIndirectPatchAddRegisters(0x…dc8) -> 0x0 from 0x…42ecd
just before: libc::memcpy(0x…f068) src 0x6000007fc2f8 n 0x8 -> …  from 0x…42ebd
just before: libSceAgc::sceAgcSetCxRegIndirectPatchAddRegisters(0x…dc8) -> 0x0 from 0x…42ecd
```

And the call census shows the whole loop: **ten `memcpy`s from the same site `0x42ebd`**, to
consecutive eight-byte destinations `…f028, …f030, …f038, … …f068, …f070`. Each iteration pairs a
`memcpy` with a `sceAgcSetCxRegIndirectPatchAddRegisters`. This is the guest's register-group build
walking a container of group descriptors (worklogs 738, 739), now visible directly in the trace rather
than reconstructed from disassembly.

## The shape that reframes it: nine small, one large

The nine copies that **succeed** move `n 0x8` — a single qword — from a **stack** source
(`0x6000007fc2f8`). The tenth, which **faults**, moves `n 0x50` (eighty bytes) from `src 0xa8`, a
null-page pointer. So the faulting group is not "the same copy as the others with a null pointer"; it is
a **different, larger, heap-backed group**. Groups zero through eight are eight-byte register writes with
their data staged on the stack; group nine is an eighty-byte block whose data lives behind a descriptor
whose data pointer is null, so `0x42d90` reads the field at `+0xa8` of a null base and faults at `0xa8`
(reconciling 738's null container with 740's `src=0xa8` — the `0xa8` is a field offset inside the null
object, and 741's tooling made both legible on one run without the register-dump misread that cost this
wall eleven ticks).

## The AGC patch call is measured-correct, so it is not the gap

The tempting fix, seeing `sceAgcSetCxRegIndirectPatchAddRegisters` in every iteration, is to make it do
more. The measured knowledge forbids it. `libSceAgc.toml` records this call from an obSCEne sweep
(20260920-110931): it **amends an already-written packet in place and writes nothing to workload
memory** — *"changed-after-set-addr 0x0, changed-after-add2 0x0; the register data is populated
caller-side"* — and returns `0x0`. orbistoun's handler is `agc_patch_returns_ok`, the shared
measured no-op for the whole `sceAgc*Patch*` family, and the trace's `-> 0x0` matches the hardware
return. So this call is faithful; making it populate the descriptor would be a hack that diverges from
the measured device to paper over the wall. The gap is caller-side, in the guest's inlined producer that
built the descriptor — exactly where 739 placed it.

## Sharpened target and the next handle

The question is now specific: **why does group nine — the large, heap-backed group — have a null data
pointer, when groups zero through eight have valid stack pointers?** The producer that fills the
descriptor is inlined (739; the title runs its own copy, so `sceAgcDcbSetCxRegistersIndirect` is not
called on this path), and it consumes the unnamed `libSceAgc::0x7d86501b8094ef57` GetSize (the phantom,
which writes a byte size and returns `0x0`; its written value was ruled out as the source of the null in
740). The productive next step is 739's, now with a live trace to anchor it: `ORBISTOUN_PEEK` the fault
stack (`rsp = 0x6000007fc018`) to recover the shared graphics object saved in the outer `0x38xxx` frame,
follow its container at `[r13+0x38]` through the accessor `0x3fdd0` to group nine's descriptor, and read
`[desc+0x18]` (the null data pointer) and `[desc+0x5b]` (its present flag). With that heap address a
watchpoint on `[desc+0x18]` catches whether anything ever writes it — the decisive one-bit test for
whether the producer skips group nine's allocation entirely, and which orbistoun-provided value it read
to do so.

## Gate state

No code changed — a run and its report only; the `orbistoun-report` change that made this legible landed
in 741 and is green. `./bin/orbistoun check` remains red on the same three generated-doc drifts from a
prior session's uncommitted `compat/PPSA02664-app0.toml` edit (740, 741), not this tick. Identity scan
clean. No commit.
