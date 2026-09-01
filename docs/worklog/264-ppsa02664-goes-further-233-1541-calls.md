# 2026-09-01 - PPSA02664 goes FURTHER (233->1541 calls): policy region was on the guest's heap


Ran the resident titles (user-directed). PPSA02664 walled at its C++ allocator (`image+0xafcc08`, null
write) with the guest printing `tlsf_add_pool: Memory size must be between 0x28 and 0x100000000`. Traced it
to root cause.

Implemented `sceKernelMprotect` (was a stub) - re-protects against the `mappings()` space, keeps the range
readable, ignores the undecodable GPU bits (D443). Needed, but did not move the wall alone.

The wall was `POLICY_REGION_BASE == 0x5000_0000_0000` - orbistoun's stub-policy region base - landing on
exactly the address PPSA02664's allocator reserves for its heap arena (the `sceKernelReserveVirtualRange`
hint). The policy region took it first, the guest's reservation fell back to the higher mapping arena, and
its arena-relative size arithmetic underflowed past tlsf's 4 GiB ceiling -> pool rejected -> null alloc ->
fault. A fresh-process VirtualAlloc test proved Windows allows `0x5000_0000_0000`, so the 487 was an
orbistoun conflict, not a VA limit. Moved `POLICY_REGION_BASE` to `0x6B…` (orbistoun's own high cluster,
clear of every guest-chosen address). Result: **FURTHER, 233 -> 1541 calls (+1307), 27 -> 37 distinct
imports.** New wall at `image+0xb14be3`: `libc::_Getpctype` (ctype table) unimplemented, answering a
placeholder the guest derefs as a pointer - the next unit (needs a citable ctype table, FreeBSD is the
oracle). Also settled a D442 aside: the full trace shows PPSA02664 reaches no flexible-memory call at all.
Tests pass (kernel/service/mem), clippy clean on touched crates, new code fmt-clean.

