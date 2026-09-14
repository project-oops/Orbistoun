# 533. Fourteen AGC packet sizes measured, and what a rival's table agreed and disagreed with

**2026-09-14** - reading obSCEne sweep 20260914-100833 (eboot leg) against prosper's public audit

## What arrived

obSCEne's sweep carried the twenty AGC checks queued for it - sixteen `*-getsize` invariants and
four argument discriminations - and the probe side had also prototyped and wrapped a large batch of
the builders that were present but never called. The result is the first set of **packet sizes read
off the real library** rather than inferred from anything.

The instrument is better than "a number": each check calls `sceAgc<Name>GetSize`, then calls the
builder through a writer obSCEne owns, and reports the reservation, the cursor advance and the write
extent **separately**. On every builder measured, all three agree. A single number could be a
coincidence; three numbers from three code paths agreeing is the library telling you the same thing
three ways.

They ran on a leg reporting `no GPU library among loaded modules`, which is now the third
confirmation that these builders are pure encoders into a caller buffer and need no driver init.

## The sizes

| builder | bytes | dwords |
|---|---|---|
| `sceAgcCbNop` | 4 × n | n (argument-driven, measured linear) |
| `sceAgcDcbDmaData`, `sceAgcAcbDmaData` | 28 | 7 |
| `sceAgcDcbAcquireMem`, `sceAgcAcbAcquireMem` | 32 | 8 |
| `sceAgcCbQueueEndOfPipeAction` | 32 | 8 (reservation only - builder absent on this leg) |
| `sceAgcDcbSetIndexCount` | 8 | 2 |
| `sceAgcDcbSetUcRegisterDirect` | 12 | 3 |
| `sceAgcDcbStallCommandBufferParser` | 8 | 2 |
| `sceAgcDcbRewind` | 8 | 2 |
| `sceAgcDcbJump`, `sceAgcAcbJump` | 16 | 4 |
| `sceAgcDcbDrawIndexIndirect` | 20 | 5 |
| `sceAgcDcbGetLodStats` | 20 | 5 |
| `sceAgcCbBranch` | 56 | 14 |
| `sceAgcDcbDrawIndexIndirectMulti` | 64 | 16 |

17 AGC knowledge entries became 31; 11 `measured` became 25.

## The cross-check, which is the point of the entry

prosper publishes a packet-size audit derived from **guest reservation behaviour** - watching titles
run out of command-buffer space - which is a completely different route from reading what the
library answers. Eight rows agree exactly. The reasoning for treating that as corroboration rather
than as a source, and for leaving the disagreements open, is D683.

Two results are worth repeating here.

**The epilogue arithmetic closes.** Their audit reconstructed a 16-dword submit epilogue from one
title's reservation and argued its occupants must be `AcquireMem` plus the end-of-pipe action - the
reasoning that forced their `RELEASE_MEM` from 9 dwords to 8 after it cost them a title. Measured
here: `AcquireMem` reserves 8, the end-of-pipe action reserves 8. 8 + 8 = 16. They inferred a pair
and measured neither; both halves are now measured and the sum is their epilogue exactly.

**`DcbJump` is where copying would have hurt.** They emit 5 dwords; the library reserves **4**. Their
own audit lists the row as oversized-by-one and open, the extra dword being private state they write
into the packet afterwards. Had the table been adopted, orbistoun would have inherited a defect its
source already knows about - and by their own analysis it is the dangerous kind, because a guest that
inlines the size never calls `GetSize` and cannot be corrected.

## Surprises

**A "contradiction" dissolved into an under-measurement.** `DcbWaitRegMem` looked like a flat conflict
- they publish 9 dwords with high confidence, we captured 56 bytes as three packets. The argument
discrimination explains it: called with zeroed arguments the builder writes 56 bytes, called with
sentinels it writes **0**. It is argument-sensitive, so the 56-byte capture is what all-zero arguments
produce and says nothing about what a title produces. Both figures can stand.

**An instrument overstated its own result, and it was ours.** obSCEne reports that check as *"the
packet length depends on the argument values"*. Its measurement is 56 versus 0, which is the shape of
a **refusal**, not of a shorter encoding. The knowledge entry records the measurement and notes the
gloss rather than repeating it. Principle 3's rule - a message naming a cause must come from the
branch that determined it - applies to a sibling's probe as readily as to our own stubs, and this is
the first time it has been applied across the repository boundary.

**The schema caught me writing a contradiction.** Fourteen entries were first written as `measured`
while carrying "only the size is measured; the purpose is read off the name" as an *assumption*.
`orbistoun-hle`'s own test rejected every one: a measured entry may not carry open questions, because
an open question puts the function back on the probe queue to re-ask something already settled. The
caveat was right and the slot was wrong - it is scope, not a worklist, and it now lives as prose in
the entry. A gate that refuses a plausible-looking record is worth more than one that files it.

## What this does not do

It does not implement a single builder. A size is what to reserve and how far to advance, not what to
write; the packet **bodies** for these fourteen are still unmeasured, and the three open divergences
stay open. The next lever is a capture with plausible arguments rather than zeroes - which is a probe
request, not a deduction.
