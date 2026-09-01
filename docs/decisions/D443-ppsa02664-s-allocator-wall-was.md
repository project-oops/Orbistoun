# D443 - PPSA02664's allocator wall was orbistoun's policy region sitting on the guest's heap


**measured** - 2026-09-01 (user-directed, /loop; running the resident titles)

Running PPSA02664 walled with a null write at `image+0xafcc08` - its C++ allocator - preceded by the guest
printing `tlsf_add_pool: Memory size must be between 0x28 and 0x100000000 bytes`. Two changes came out of
tracing it; the second is the one that moved the wall.

**`sceKernelMprotect` implemented.** It was a bare stub. The guest reserves a span, then `mprotect`s it
usable before handing it to its allocator; a placeholder answer left the range looking unusable. The
handler re-protects against the same `mappings()` space the reservation used (`AddressSpace::protect`
refuses a range it does not own, so a typo cannot touch this process's own code), reusing the existing
`protection_from_guest` decode. Two honest simplifications, both from the identity-mapped model rather than
guessed: the range is kept **readable** (the `0xf2` a guest passes decodes to write-without-read, which
would drop a range the allocator must read back), and the high GPU/cache bits are **ignored, not decoded**
(no citable layout; inventing one is the D008 error). Implementing it did **not** move the wall on its own -
useful to know, and it is needed regardless - which is what pointed at the reservation itself.

**The real cause: `POLICY_REGION_BASE` collided with the guest's heap arena.** PPSA02664's allocator
reserves its arena at a fixed hint - `0x5000_0000_0000` - through `sceKernelReserveVirtualRange`, and does
its size arithmetic relative to that address. orbistoun's stub-policy regions were based at *exactly*
`0x5000_0000_0000` (chosen as "empty", clear of loader/stack - but nobody had checked it against where a
guest's own allocator lands). Reserved before the guest ran, the policy region took the address first;
`VirtualAlloc` at the hint then returned `ERROR_INVALID_ADDRESS` (a fresh-process test confirmed Windows
itself allows the address, so the 487 was an orbistoun-owned conflict, not a VA limit); the reservation fell
back to the mapping arena at `0x72…`, a *higher* address; and the guest computed
`pool_size = arena_end - returned_base`, which underflowed to a value past `tlsf`'s 4 GiB ceiling. `tlsf`
rejected the pool, the next allocation returned null, and the guest wrote through it.

Moving `POLICY_REGION_BASE` into orbistoun's own high cluster (`0x6B…`, between the TLS block and the
mapping arena, where nothing a guest chooses lands) freed the hint. The reservation is now honoured, the
guest's arithmetic works, `tlsf` accepts the pool, and **PPSA02664 goes FURTHER: from 233 calls to 1541
(+1307), 27 to 37 distinct imports**. The lesson generalises past this title - orbistoun places several
fixed regions at guessed addresses (the sentinel already learned this and picked an odd base); a guest's own
allocator is one more claimant to stay clear of, and "clear of *our* regions" was not the same as "clear".

**The new wall, for the next unit.** The guest now faults at `image+0xb14be3` dereferencing `0x7fff00cf` -
one of our own placeholder codes used as a pointer. `libc::_Getpctype` (the ctype-table accessor, part of
locale/character classification) is unimplemented and answered the placeholder; `malloc_stats_fast` is
unimplemented too. `_Getpctype` returns a pointer to a character-classification table, so it needs a real
implementation (a citable table - FreeBSD's ctype is the oracle), not a policy region. That is the next wall
past the allocator.

**Settles the D442 aside.** The full trace shows PPSA02664 reaching no flexible-memory call of any kind, so
the recorded "imposing the flexible figure took it backwards" cannot have been the size query - consistent
with D442's finding that it imports no flexible function. It sizes off `sceKernelGetDirectMemorySize` and the
arena reservation, both now answered.

