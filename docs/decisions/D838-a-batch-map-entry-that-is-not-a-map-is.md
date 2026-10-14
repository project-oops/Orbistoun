# D838 - A batch-map entry carries out its operation, as measured

**Status:** decided
**Date:** 2026-10-14

`sceKernelBatchMap` reads each 32-byte entry's four bytes at +28 as its operation, and carries out
each entry in order: the maps before an entry of another kind are placed before it acts.

- **0** maps direct memory at the entry's address.
- **1** unmaps the range, as `sceKernelMunmap` does: the alias goes, and a placement wholly inside
  it is released, so a later map at that address - a longer one included - places.
- **2** gives a range already mapped whole the entry's protection, keeping its contents.
- **3** maps zeroed memory at the range with the entry's protection, over what was there.

Any other operation, and a protect of a range not mapped whole, is refused: the entries before it
are carried out and counted in `completed`, it and those after it are not, the call answers the
unimplemented placeholder, and the run names the entry, its range and the operation.

**Why:** REQ-20261014T0230Z-bm01 measured each operation on 64 KiB of direct memory, the neighbours
watched (`report-20261014-batch3.log` 5971-6030): 1 left the range answering `0x8002000d` to a
query and faulting on a read, the neighbours intact, and a later map placed it again; 2 left it
readable at protection `0x11`; 3 left it mapped at `0x33` reading zero; 4 left it reading what
it held at `0x33`. Grand Theft Auto V streams through operation 1 beside maps of the same addresses.
Until this was read, every entry was mapped whatever it asked for; until it was measured, an
entry of any other operation was refused.

**Open:** operation 4 kept the contents and the protection the probe gave, which was the range's
own, so what it changes is not separable from doing nothing. It stays refused. A partial unmap
keeps a placement that is only partly inside the range, as `sceKernelMunmap` does (D749), where
the hardware leaves the rest mapped and the range unmapped.

**Rejected:**
- Ignoring the field, as before: a stub answering success.
- Treating 4 as a map, a protect or nothing: one probe cannot tell them apart.
