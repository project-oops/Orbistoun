# D757 - A loaded segment with no access flags is read-only

**Status:** assumed
**Date:** 2026-10-07

A `PT_LOAD` whose `p_flags` grant none of read, write and execute is mapped read-only. Every
other combination maps as its flags say, execute-only text included.

**Why:** PPSA04263's eighth `PT_LOAD` (`0x5e69670`, `0x40e8c8` bytes from the file) has
`p_flags` 0 and holds the title's dynamic table: its `PT_DYNAMIC` ends where the segment does.
That is where the Prospero generation maps its dynamic tables (SELFish writes them as a mapped
`PT_LOAD` for the same generation). The title reads the segment with `movbe` early in its
start-up, and a retail title runs on the console (D708), so the console maps it readable.
Mapped no-access, it faulted at its first page. Known by the guest, not by a measurement: no
console row says which access the page has, only that the title's read of it succeeds.

**Rejected:**
- Mapping it read-write. Nothing has been seen writing it, and read is all the guest shows.
- Taking the access from the `PT_DYNAMIC` header inside it (`p_flags` 6). That header places no
  memory, and a no-access segment holding no dynamic table would still fault.
