# D446 - sceKernelVirtualQuery now sees the guest's own image and stack


**measured** - 2026-09-01 (user-directed /loop; obSCEne oracle)

Two of obSCEne's failures under orbistoun (D445) were `020-memory/virtual-query-text` and
`virtual-query-stack`, both `0x80020002` - "virtual query on code/stack address refused". `sceKernelVirtualQuery`
looked up the address only in the kernel's runtime map (`mappings`), which holds what the guest allocated at
run time. The loaded image, the stack and the main-thread TLS block live in address spaces the loader and
worker own; `mappings` never sees them, so a guest querying its own code or stack was told nothing is there -
where a console answers with the mapping.

Fixed by giving the query every source of guest-readable regions. A small `note_region(base, len)` registry
records regions this crate did not map itself; the worker notes the **image span** into it (the stack and
loaded modules were already recorded via `note_stack_span`/`note_loaded_modules` for `sceKernelIsStack` and
`sceKernelGetModuleList`). A new `region_containing(addr)` consults, in order, the runtime map, the noted
regions, this thread's own stack, and the main stack span - and both `virtual_query` and the stack test now
go through it. One lookup, every place a legitimate address can live.

Verified against the oracle: obSCEne's `virtual-query-text` now passes (`0xa2b000`) and `virtual-query-stack`
passes (`0x800000`), where both failed before; obSCEne's failure set drops from seven distinct to five, with
no new failures. A unit test pins the registry (found inside, exclusive at the end, not invented in a gap,
idempotent on re-note, cleared on reset). Kernel/worker/mem tests pass, clippy clean on the new code,
new code fmt-clean.

**Not this fix, but named by it:** `virtual-query-unmapped` reports `partial` - it queries the fixed address
`0x720000240000`, which lands in orbistoun's mapping arena (`MAPPING_BASE`), so obSCEne's own reservations
under orbistoun reach it and the query answers "mapped" where the console refuses it (`0x8002000d`). This
predates this change (it was already `partial`) and is a separate divergence about where the arena places
things, not about what the query can see. Left for its own turn.

**The three failures still standing** (D445's list, minus the two fixed here): `110-modules/info-size` and
`/names` (the one-module gap), `135-sysctl/osrelease` and `137-kernelcall/system-version` (both refused, and
both measured on hardware - answerable from the configured machine rather than invented), and
`900-surface/control` (the resolver reports a symbol that does not exist as present).

