# D566 - The biggest item in the work list has no name, and no return value fixes it

**Status:** measured
**Date:** 2026-09-04

## The number

`orbistoun-cli worklist` ranks every import across every run this machine has recorded. The top
entry is not close:

```text
      CALLS  SHARE  MODULES  IMPORT
   11272991  78.3%        1  libkernel_sync_on_address::0xbd04891e6902ce1d
     733170   5.0%        9  libkernel_fs::sceKernelWrite
```

**Seventy-eight percent of every guest call this project has ever recorded** - 11,272,991 of
14,379,710 across 65 runs - goes to one function, in one title, and **it has no name**.

It is also 97% of PPSA25872's entire run: 11.27M of 11.58M calls, which is why that title records
a `standing` of 3%.

## What the guest is doing

Calling it on **one address** - `0x740000754a38`, in its own heap - eleven million times, receiving
the placeholder each time. That is a hard spin on something that never resolves.

The library name is the strongest evidence available: `libkernel_sync_on_address` is the futex
family, so this is a wait, a wake, or a timed variant of one.

## No return value fixes it, and that is measured

The obvious experiment is to answer success. `ORBISTOUN_RETURN=0xbd04891e6902ce1d:0x0` makes it
**worse**:

| | placeholder | answering 0 |
|---|--:|--:|
| calls | 11,583,204 | **19,999,997** (budget exhausted) |
| imports | 129 | **78** |

So the guest does not proceed on success; it spins harder and reaches less of the interface. The
same shape as `sceAgcCreateShader` (D556): **the mechanism has to be modelled, not answered.** A
wait that returns without waiting is a busy loop by construction, and no constant makes it not one.

## Why it is not implemented here and now

**Wait and wake need opposite behaviours**, and nothing establishes which this is. Implementing
the wrong one is not a slow emulator, it is a guest whose synchronisation is inverted - and with
one caller and one address, the run would look plausible either way.

Naming it is the whole of the blocker. `orbistoun-cli names` proves a name by hash rather than
guessing it, so a hit is proof and a miss costs only the sweep. **It was run, and it missed:**

```text
module strings:    29,689 candidates,  0 named
published names:    3,018 tried,       0 named
generated names:    3,794,810,244 candidates across 11 patterns,  0 named
```

Seven hand-tried candidates missed first - `sceKernelWaitOnAddress`, `sceKernelWakeOnAddress`,
`sceKernelWaitOnAddressWithTimeout` and four variants.

Per the command's own contract, *a miss proves only that the name was not among those tried*. Three
and a half billion candidates is a strong miss all the same: the vocabulary does not generate this
family, and extending it blindly is not a plan.

(The run's warning that a total miss on published names *"is a strong sign --suffix-hex is wrong"*
does not apply - 129 other imports in the same module resolve, so the suffix is right and the
heuristic is firing on a module whose unnamed remainder is simply vendor-specific.)

## What neither repository knows

`0xbd04891e6902ce1d` appears **nowhere in obSCEne**, and no hardware report mentions
`sync_on_address` at all. orbistoun declares no such library either. This is a family both
projects have entirely overlooked, and it accounts for more guest calls than everything else
combined.

That makes it a clean ask: **census `libkernel_sync_on_address`**. Its surface is small, a name is
proved by hash rather than believed, and one name retires the largest single item in the work list.

## What this does not establish

**That it is a wait rather than a wake.** The library name says the family and the call pattern -
one address, one caller, eleven million times - reads like a wait, which is inference from
behaviour and not a measurement.

**Nor that naming it is sufficient.** A named wait still needs a model in which something can
change the address and wake it; naming only makes it possible to build the right one.
