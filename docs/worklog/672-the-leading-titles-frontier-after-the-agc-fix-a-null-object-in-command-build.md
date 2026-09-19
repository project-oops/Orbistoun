# 672. The leading titles' frontier after the AGC fix: a null object in command-buffer build

**2026-09-17** — a full local sweep, the first run of the corpus since the `sceAgcDcbSetCxRegistersIndirect`
placeholder fix (worklog 616) landed in-tree. It answers the question worklog 616 left open ("whether
they then reach a *new* wall needs a run"): **the placeholder wall is cleared, and a new one sits one
step behind it.**

## The fix worked; the fault moved

PPSA02664 and PPSA03416 both still crash in `memcpy` at `VCRUNTIME140.dll+0x1dc8d`, but the faulting
read **changed from `0xf7ff0001` (the old `0xf7ff0001` placeholder, worklog 594) to `0xa8`**. That
shift is the proof the fix took: the patch chain no longer carries orbistoun's placeholder into the
copy. `sceAgcSetCxRegIndirectPatchAddRegisters` now returns the measured `0x0` all through the loop.

## The new wall: a null object, field `+0xa8`

The run's own diagnosis pins it. At the fault `r13 = 0x0` and `r14 = 0x0`, `rdx = 0xa8`, and the read
is `base + 0xa8` where the base register is zero - "a field read through a pointer that was zero". The
loop just before is, repeated, dest walking forward by 8 each pass:

```
sceAgcSetCxRegIndirectPatchAddRegisters(0x…87dc8) -> 0x0    from image+0x42ecd
libc::memcpy(0x…8f068) -> 0x…8f068                          from image+0x42ebd
sceAgcSetCxRegIndirectPatchAddRegisters(0x…87dc8) -> 0x0    from image+0x42ecd
libc::memcpy(0x…8f070)   ← faults, source is a null object + 0xa8
```

So the guest is copying, element by element, out of a list of objects into a growing buffer; the
object at the fault is **null**, and the field it reads (`+0xa8`) becomes a `memcpy` source of `0xa8`.
Something that should have produced that object returned zero.

## The concrete gap right at the wall

The run flags an **unnamed libSceAgc import in the same spot**: `libSceAgc::0x7d86501b8094ef57`, called
twice, with the signature `(ptr, ptr, 0, ptr, u32, u32)`. Its arg1 points at a real command buffer
(`0xffff1000 0xc0055000 …` - a NOP filler then a `DMA_DATA` header), and its arg3 is `0x7ff7c3e145f0`,
a **host** address outside every region the run gave the guest. An unnamed import is one whose NID
resolved to no name in the database, so orbistoun neither names nor implements it - it answers the
`Unimplemented` placeholder. A libSceAgc call operating on the command buffer, unimplemented, right
where a null object appears, is the most likely producer of that null: **name and implement it and the
null may be filled.**

This is the operative "GfxDevicePS5SharedData::CreateWorkload()" path - the guest's own TODO strings
in the same run name it (`todo: void GfxDevicePS5SharedData::CreateWorkload()`), which is Unity
building a GPU workload (command buffer) at startup.

## What this means for the renderer question (36c0)

The crash is during command-buffer **construction**, upstream of `sceGnmSubmitCommandBuffers`. So for
these two titles attaching a renderer would not help yet - they never reach submit. The current
frontier is this AGC null object, not the (deliberately declared-only) submit path.

## Filed

The bounded next step - a name for NID `0x7d86501b8094ef57` in libSceAgc - is filed to obSCEne
(`REQ-…-7c21`), since naming a vendor NID clean-room is a sibling job and obSCEne can read the
module's own export table on hardware. Not resolvable from inside orbistoun without pulling an
emulator's symbol list across the provenance boundary.

## Other titles (unchanged in character)

PPSA04263 `image+0x196b91a` (read -1), PPSA21564 `own modules+0x7af792` (read 0x38), PPSA25872
`image+0x17554a3` (read -1), PPSA28061 `abort`, obscene-payload clean `exit(0)`. All the surviving
Unity titles fault reading a field off a null-or-`-1` object - the same shape, a stubbed call
returning `0`/`-1` where a real handle was due.

## Gate state

Diagnostic only - no code change this unit. The sweep and run are the artifacts. No commit.
