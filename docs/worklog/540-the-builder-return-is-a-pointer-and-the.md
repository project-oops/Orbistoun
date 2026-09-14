# 540. The builder return is a pointer, and the run that showed it

**2026-09-14** - a negative result from the loop, and a correction it forced

## The run

`./bin/orbistoun run PPSA02664-app0`, with the eight builders from worklog 538 newly wired. The
honest headline: **the wall did not move.** Same fault, same address `0x7fff13abdc8d`, 418419 calls
before, 418420 after.

What did change is `unanswered 35 -> 34`. Exactly one import newly answered - `sceAgcDcbEventWrite`,
visible in the trace returning a real value where it used to return `0xf7ff0001`. The other seven are
not exercised by this title. So the wiring is confirmed working end to end in a real guest, and
confirmed insufficient for this one.

## What the trace gave away

```
sceAgcDcbAcquireMem(0x740002387868)                  -> 0xf7ff0001
memcpy(0x74000238f028)
sceAgcSetCxRegIndirectPatchAddRegisters(0xf7ff0001)  -> 0xf7ff0001
memcpy(0x74000238f030)
sceAgcSetCxRegIndirectPatchAddRegisters(0xf7ff0001)  -> 0xf7ff0001
   ... repeating, the memcpy stepping eight bytes at a time
```

The guest takes `DcbAcquireMem`'s return and **passes it straight back in** as the first argument to
a patch function. So a builder's return is a handle the guest holds, not a status it discards.

## The correction

Worklog 538 recorded `0x200060078` - the value every builder returns, in every run - as "a constant
rather than a per-run address", reasoning from its stability. **That was wrong**, and the arithmetic
in obSCEne's own probe says so:

| step | value |
|---|---|
| `oops_mem_alloc` base | `0x200060000` |
| `(base + 64 + 63) & ~63` | `0x200060040` |
| less the eight-byte count prefix | `0x200060038` |
| plus the struct header (`0x40`) | **`0x200060078`** |

Exactly the returned value. Other allocations in the same sweep - `0x200080000`, `0x200028000` - sit
in the same region. It is a **pointer into the probe's own command buffer**, and it is constant only
because that allocator is deterministic and every check calls `agc_cb_prepare` before the builder.

Stability across runs was taken as evidence of a constant. It was evidence of a deterministic
allocator, which is a different thing, and the census of about twenty `sceAgc*Patch*` entry points -
`WaitRegMemPatchAddress`, `DmaDataPatchSetDstAddressOrOffset`, `QueueEndOfPipeActionPatchData` - was
sitting in the same log saying what the pointer is *for*: amending a packet after it is written.

`dcb_append` now returns the cursor from **before** the append, the knowledge entry is corrected in
place rather than left to be read, and the sequencing test asserts three calls return three
addresses eight bytes apart - which is the property that separates the two readings.

## What is still derived

**Which** pointer. Every obSCEne check resets the writer, so `cur == begin` at the call and "the
packet's address" and "the buffer's start" fit the measurements equally. orbistoun implements the
former because the latter would let only the first packet in a buffer ever be patched - a
derivation, and it is marked as one. `REQ-...c74f` asks for two builders called through one un-reset
writer, which settles it for the cost of not calling `agc_cb_prepare` twice.

## Surprises

**A negative result was the most informative thing today.** The run proved the wiring works and
changed nothing about the title, and the trace it printed on the way past is what found the pointer
and named the next builder. Worth remembering when a change looks too small to be worth measuring.

**I truncated my own evidence.** The run was launched through `| tail -60`, which threw away the
progress block; the before/after had to be reconstructed from the compat record instead. Do not pipe
a tool whose report is the point.
