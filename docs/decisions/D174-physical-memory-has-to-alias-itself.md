# D174 - Physical memory has to alias itself


**decided** · 2026-08-21

`sceKernelMapNamedDirectMemory` ignored its `physical` argument entirely and handed out a
fresh reservation every time.

A guest allocates a physical range, maps it, loads a file into the address it was given,
and later maps that range again expecting its data still to be there. Fresh zeroed memory
the second time is **silent, total data loss** - the guest reads zeroes out of a buffer it
filled, and the fault lands wherever it first trusts the contents, which is nowhere near
the map call.

Mappings are now keyed by physical offset: the same range answers the same address. The
physical offset is the *identity* of the memory; the virtual address is only where it is
currently reachable.

Not full aliasing, and the limit is stated rather than left to be discovered: two
*simultaneous* mappings of one range still get one address, which would need a shared
memory object rather than a reservation. The case fixed is the one that actually occurs.

**It did not move the wall**, so the hypothesis that drove it was wrong. Kept because the
bug is real whether or not this title triggers it - a silent data-loss path with no
signature is worth closing on its own account.

