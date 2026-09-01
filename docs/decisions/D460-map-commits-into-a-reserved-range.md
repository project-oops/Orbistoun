# D460 - Mapping direct memory commits into an existing reservation, it does not reserve again


**measured** - 2026-09-01 (user-directed /loop: crack the refers, immediately after D459)

PPSA02664 faulted writing to `0x0` at `image+0xafcc08`. The return column D459 added named the
cause in one run: `sceKernelMapDirectMemory` answered `0x7fff0004` - `GuestError::NoMemory` -
while `sceKernelReserveVirtualRange`, `sceKernelVirtualQuery` and
`sceKernelAllocateMainDirectMemory` all answered `0x0` (`OK`). Three of the four memory calls
succeeded and the map failed. This replaced the previous turn's guess (a wrong `tlsf` pool size,
or an address-space mismatch), which the returns disproved outright.

**The cause is reserve-then-map treated as reserve-twice.** A guest carves a virtual range with
`sceKernelReserveVirtualRange` and then places physical memory *inside* it with
`sceKernelMapDirectMemory`, at the address it was handed. `map_named_direct_memory` called
`AddressSpace::reserve` unconditionally, which validates against existing regions and returns
`MemError::Conflict` for a range that overlaps one - and the range was the reservation the guest
had just made. The map answered `NoMemory`, the guest read that as out-of-memory, kept the null
its allocator returned, and wrote through it two instructions later on the branch D458 decoded.

**The fix.** `AddressSpace::owns(base, len)` - the containment test `protect` already performed
inline, now named and shared - reports whether a range lies wholly within a region this space
reserved. `map_named_direct_memory` consults it: a range the guest **pre-reserved** is committed
into with `protect` (on orbistoun's identity-mapped model the reservation already backs the
pages, so the map is a re-protect plus the physical-alias bookkeeping), and only a range it did
*not* pre-reserve is a fresh mapping that still reserves. This is the true console semantics -
reserve carves address space, map places physical memory within it - not a workaround.

**Verified by two observations, not one (the D224/D226/D227 rule).** The wall moved
`image+0xafcc08 -> image+0xb14be3`, *and* the run went from 234 calls / 26 distinct imports to
**1544 calls / 39 distinct imports** (`+1310`, `+13`), verdict **FURTHER**. The second number is
what makes it a fix rather than a wall shoved sideways: a program cannot make thirteen hundred
more library calls without a working heap, so the allocator now initialises and the guest runs
on. `sceKernelMapDirectMemory` answering `0x0` where it answered `0x7fff0004` is the same fact
read the other way, from the very column that found it. The new wall, `image+0xb14be3`, is
`_Getpctype` returning a placeholder the guest dereferences - already named by D459 and the next
target. Provenance-clean: no guest code read; `owns` is a pure containment test with its own unit
test.
