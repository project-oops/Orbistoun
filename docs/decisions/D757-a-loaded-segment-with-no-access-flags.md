# D757 - A loaded segment with no access flags is read-write

**Status:** assumed
**Date:** 2026-10-07

A `PT_LOAD` whose `p_flags` grant none of read, write and execute is mapped read-write. Every
other combination maps as its flags say, execute-only text included.

**Why:** PPSA04263's eighth `PT_LOAD` (`0x5e69670`, `0x40e8c8` bytes from the file) has
`p_flags` 0 and holds the title's dynamic table: its `PT_DYNAMIC` ends where the segment does.
That is where the Prospero generation maps its dynamic tables, and SELFish writes them for the
same generation as a mapped `PT_LOAD` with `p_flags` 6. Early in start-up the title reads the
segment with `movbe` and writes it back in place, decrypted. A retail title runs on the console
(D708), so the console maps the segment readable and writable. Mapped no-access, the title
faulted on its first page; mapped read-only, on its first write. This is known from the guest,
not measured: no console row gives the page's access, only that the title's reads and writes
succeed.

**Rejected:**
- Read-only. The title writes the segment.
- Taking the access from the `PT_DYNAMIC` header inside it (`p_flags` 6). That header places no
  memory, and a no-access segment holding no dynamic table would still fault.
