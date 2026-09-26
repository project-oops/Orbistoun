# D588 - Guest write access implies read

**Status:** decided
**Date:** 2026-09-08

`protection_from_guest` maps a guest request for write access to a readable host protection. The
readable-range table holds 512 entries, and a range it cannot hold is counted and reported.

**Why:** an x86-64 page table entry has no read bit, so a writable page is readable on the
hardware being presented; POSIX permits granting more than was asked and FreeBSD on amd64 grants
read with write. Recording such a mapping as unreadable makes diagnostics call a valid pointer
wild. The table is fixed because it is read on the guest's own stack, where allocating is
forbidden, and a dropped range is counted so a capacity limit never reads as a bad pointer.

**Rejected:**
- Write-only as requested: stricter than the machine, and hides guest buffers from every dump.
- A growable table: allocates on the guest's stack.
- Dropping ranges silently: an overflowing run reports every pointer into them as the guest's fault.
