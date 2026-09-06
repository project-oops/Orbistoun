# 2026-09-03 - (/loop) The oscillation is settled: it was the guest's own allocator

```
guest runs     69 across 13 bases and both directions
oscillation    9-of-12 -> 0-of-51, and flipped on demand
```

Arrived by hand again - the nineteenth wakeup that did not fire.

## What was built

`ORBISTOUN_HEAP_BASE`, a diagnostic that serves every guest allocation from a bump region
reserved at a fixed address instead of from `std::alloc`. Same block shape as the host path -
D128's header, `total` bytes aligned to `align` - so `free`, `realloc` and `header_of` are
written once and never learn which allocator answered. `free` asks whether the pointer is in
the region **before** it reads the header, because both allocators write the same header and
`dealloc` against reserved memory is undefined behaviour.

The dependency edge D499 priced in does not exist: `orbistoun-libc` already takes
`orbistoun-thunk`, which already reserves at fixed bases, and principle 4 requires the
reservation to go through `orbistoun-mem` anyway.

## What it settled, in three steps

**Fixing the heap collapses the oscillation.** Twelve interleaved pairs: the host heap gave
2080/46 nine times out of twelve, the fixed heap gave 2077/44 twelve out of twelve.

**The address value is not what does it.** Thirteen bases from `0x1000_0000_0000` upward,
varying bits 16 through 45 - fifty-one runs, every one 2077. A negative, and the one that
made the next step worth taking.

**Reversing the order flips the branch.** The region grows downward instead of upward and
nothing else changes - same base, same span, same determinism, same contiguity, still never
reused. Six interleaved pairs, zero crossover:

```text
ascending   2077  2077  2077  2077  2077  2077
descending  2080  2080  2080  2080  2080  2080
```

## And the traces say what the guest is doing

```text
only in 2080   sceKernelAllocateMainDirectMemory, sceKernelMapDirectMemory,
               sceKernelSetVirtualRangeName, a fourth sceKernelDirectMemoryQuery
only in 2077   sceKernelMprotect
```

A heap deciding whether it can grow in place. Block above what it holds - extend the range,
one `mprotect`. Block below - take a fresh direct-memory segment, map it, name it. The host
allocator settles that relationship differently run to run because it reuses freed blocks,
and the whole ~40/60 split was that.

**So `-down` is now the cheapest way to reach the direct-memory path** on a title that
otherwise never asks for it: three functions and a query, one environment variable away.

## Three surprises worth keeping

**The first base broke the run, and the address map is why.** `0x7400_0000_0000` looked clear
of the module base and the thunk table. It is `orbistoun_kernel::MAPPING_BASE`, which
`sceKernelReserveVirtualRange` hands out from - sixteen refused reservations and a fall from
2077 calls to 219. The map was twenty bases across nine crates with no document to check first.
The default moved into the `0x0000_5E2*` family this project already keeps for regions of its
own invention, and the map is now `docs/ADDRESS_MAP.md`, gated by a test that walks the tree
and refuses two bases within four gibibytes - watched refusing this exact collision at a
distance of zero.

**Every intervening diagnostic was silent on the path that matters.** `heap_fill_summary` and
`direct_fill_summary` printed from `summarise_calls`, which only a timeout and an exhausted
budget reach. A fault reached neither - the most common ending this project has, and the one
they exist to explain. Moved to `persist`, which every ending comes through.

**The verdict was reporting an allocator strategy as progress.** The 2080 branch reaches three
imports the 2077 branch does not, so the report calls it `FURTHER`. Both branches fault at the
same instruction on the same read of `0x8`. Nothing got further; different code ran.

## Both halves were broken and watched to fail

```text
allocate ignores the region   -> first block at 0x2a3... , not base+16   FAILED
free skips the range check    -> STATUS_ACCESS_VIOLATION (0xc0000005)
```

The descending direction is an instrument, so it has its own test asserting it really
descends - a region that failed to reverse and a region that reversed and changed nothing are
the same output otherwise (check 3).

## What is not established

Which branch the console takes. Both are paths the guest has; the platform's own allocator
picks by a rule nobody has measured. What orbistoun has now is the ability to choose one
deterministically, which every later experiment against this wall needed.

`scePthreadSelf` still answers a host address that differs between runs. Fifty-one runs say it
is not what decides this branch, but it is a second place a guest can read the host's layout
and it is untouched.

Decision: [D513](../decisions/D513-the-oscillation-was-the-guests-own-allocator.md).
