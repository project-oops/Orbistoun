# 600. The patch-family producer is wired, and the retail wall is a cluster

**2026-09-15** - obSCEne resolved five requests overnight, one of them the retail wall's, and
acting on it revealed the wall is deeper than one function

## What arrived

`REQ-...4386` came back RESOLVED, with `166-agc/patch-cx-registers-indirect`:

- Producer `sceAgcDcbSetCxRegistersIndirect`: returns the packet-start pointer, cursor delta
  `0x14` (20 bytes), header `0xc0039f00`.
- Patch `sceAgcSetCxRegIndirectPatchAddRegisters(pkt_ptr, ...)`: returns `0x0`, does **not**
  advance the cursor, amends the packet body in place (one measured byte flipped).

Four more resolved in the same sweep (`72d7`, `ae35`, `c74f`, and the NGG field split), which are
recorded against their own requests.

## What was wired, and the discipline around it

`sceAgcDcbSetCxRegistersIndirect` is now the seventeenth wired builder. It is **different in kind**
from the other sixteen and the code says so: it reserves the measured 20-byte extent and writes
only the measured header, leaving the four body dwords zero.

That is not laziness, it is the honest-failure line. `REQ-...4386` is one before/after, not an
argument sweep, so it fixes the header and the extent but **not** which argument becomes which body
dword. Writing obSCEne's captured body values would encode obSCEne's arguments into the guest's
stream. The body is the guest's to fill through the patch family; the reservation is the part that
must be right, and it is the part that is measured (D696).

## It works, and it was not enough

The producer now hands the guest a real cursor where an unwired builder handed it the loud
placeholder. Proven, not assumed - the patch's first argument moved:

```
before:  sceAgcSetCxRegIndirectPatchAddRegisters(0xf7ff0001) -> ...
after:   sceAgcSetCxRegIndirectPatchAddRegisters(0x740002107d9c) -> ...
```

`0x740002107d9c` is a real guest mapping. The placeholder-as-pointer that worklog 553 and 594
described is gone from the Cx family's data flow.

**The wall did not move.** Still `VCRUNTIME140.dll+0x1dc8d`, verdict `same`. The fault is
`read of 0xa8` with `r13 = r14 = 0` - a field read at `+0xa8` off a null pointer, byte-identical
to the pre-fix run. So the memcpy that faults was never the Cx patch's; it is downstream, and it
is a different null.

## The wall is a cluster, which the call tail shows

The last calls before the fault are a whole family of unimplemented builders on a **secondary
command buffer** (arg0 `0x6000007fbe58`, a stack address, not the Cx handle):

```
sceAgcCbNop(...)              -> 0xf7ff0001
sceAgcDcbDmaData(...)         -> 0xf7ff0001
sceAgcCbReleaseMem(...)       -> 0xf7ff0001
sceAgcDcbWaitRegMem(...)      -> 0xf7ff0001
memcpy(0x740002107dd8)        -> real
0x7d86501b8094ef57(...)       -> 0xf7ff0001   (unnamed)
sceAgcDmaDataPatchSetDstAddressOrOffset(0x1400f98fbf49) -> 0xf7ff0001
sceAgcWaitRegMemPatchAddress(0x1400f98fbf49)            -> 0xf7ff0001
```

**The same producer/patch pattern repeats across families.** DmaData and WaitRegMem each have a
producer (unimplemented, answering the placeholder) and a patch (carrying `0x1400f98fbf49`, a
pointer computed from a placeholder). Fixing the Cx producer fixed one of several; the others still
hand their patches bad pointers.

## Why not wire the rest now

Because they are not equally measured, and the loop rule is measure-first:

- `sceAgcDcbDmaData`: extent 28 bytes is measured (`REQ-...d3cb`), body encoding is not.
- `sceAgcCbNop`: measured whole - `0xffff1000`, four bytes (`166-agc/cb-nop`), already known to
  `packet::walk`. Wireable.
- `sceAgcCbReleaseMem`: not in any resolved request read.
- `sceAgcDcbWaitRegMem`: `REQ-...72d7` measured its Acb twin at **zero arguments** (56 bytes), and
  with real arguments the builder *refused execution* on sentinel register addresses - so its
  argument-to-body mapping is explicitly unmeasured. Not implementable.

So the cluster needs one more probe before it can be cleared: the producer packet bodies for
DmaData, ReleaseMem and Nop, the way `REQ-...4386` did for Cx. Filed as `REQ-...` on the bus.

## What was refused

- Encoding the Cx body from one capture. Documented and zeroed instead.
- Wiring DmaData/WaitRegMem producers on the strength of an extent alone, or of a zero-argument
  capture. The `read of 0xa8` fault will not be cleared by a producer that reserves the wrong
  length or writes a body from guessed arguments, and it would be indistinguishable from one that
  reserves the right length - so the probe comes first.

## What moved on the record

Both retail titles now serve one more of their imports by an implementation - PPSA02664 189 -> 190
answered, PPSA03416 192 -> 193 - because the producer is answered rather than stubbed. Reach and
fault are unchanged; the frontier was regenerated to reflect the higher answered count and the
reproducible fault name from worklog 594.

## Gate state

fmt clean, clippy `--workspace --all-targets -D warnings` clean, `cargo test --workspace` 2,340
pass / 0 fail (the wired-set guard from worklog 589 correctly demanded 16 -> 17), worklogs unique,
identity scan clean.
