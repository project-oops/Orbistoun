# 553. The AGC patch family is one producer and its amendments

**2026-09-14** - the wall both leading titles share, characterised but not implemented

## What it is

`sceAgcSetCxRegIndirectPatchAddRegisters` is the top unimplemented import for **both** PPSA02664 and
PPSA03416, and they die at the same address, `0x7fff13abdc8d`. It is the only wall they have in
common.

It is not a builder. The guest's call counts say so outright:

| producer | called | patched by | called |
|---|---:|---|---:|
| `DcbSetCxRegistersIndirect` | 1 | `SetCxRegIndirectPatchAddRegisters` | **32** |
| `DcbSetShRegistersIndirect` | 1 | `SetShRegIndirectPatchAddRegisters` | 1 |
| `DcbSetUcRegistersIndirect` | 1 | `SetUcRegIndirectPatchAddRegisters` | 3 |
| `DcbDmaData` | 2 | `DmaDataPatchSetDstAddressOrOffset` | 2 |
| `DcbWaitRegMem` | 3 | `WaitRegMemPatchAddress` | 2 |

One packet is written, then amended repeatedly. That is what the whole `sceAgc*Patch*` family is for,
and it is why D686's finding - a builder returns the address of the packet it wrote - is load-bearing
rather than trivia: the return **is** the handle the patch functions take.

## What the guest gives for free

Two things worth having, both read off the trace rather than reasoned out:

- **arg0 is the packet, and it is currently our placeholder.** `DcbSetCxRegistersIndirect` is
  unimplemented, so it answers `0xf7ff0001`, and that exact value arrives as the patch function's
  first argument on all thirty-two calls. The guest is carrying orbistoun's placeholder as a pointer
  and faulting later inside `memcpy` - the leak D670 anticipated, which a loud value cannot prevent
  when the guest never checks it.
- **Eight bytes per call.** Each patch is immediately preceded by a `memcpy` from the same loop body
  (`image+0x42ebd`, then `image+0x42ecd`) whose destinations step `f028, f030, f038, f040` - eight
  apart. One 8-byte entry written, one patch per entry.

Both recorded as `guest-observed`, which is exactly what they are.

## Why it is not implemented

All twenty `sceAgc*Patch*` symbols are **`absent`** in every census `166-agc` has run, and so is
`DcbSetCxRegistersIndirect`. Nothing has ever called one, so nothing has measured either the
producer's packet body or what a patch does to it.

Implementing would stack two guesses, and the second is the dangerous one: it amends the first, so a
wrong patch is invisible in the packet a wrong builder produced. That is the shape principle 3 refuses,
and the reason is specific rather than cautious - `DcbDrawIndex` was left out for one unmeasured dword
pair, and this is worse.

`REQ-20260914T1730Z-4386` asks for the one experiment that settles it: build a packet, patch it, and
**dump the buffer both times**. The diff is the whole answer, and neither dump alone contains it.

## Surprises

**The family is bigger than the wall.** Chasing one function found twenty, in five producer/patch
pairs - and two of those producers are already measured (`DcbDmaData` fully, `DcbWaitRegMem`
zero-argument). So the same probe shape unblocks more than the title that prompted it, which is worth
knowing before writing the probe rather than after.
