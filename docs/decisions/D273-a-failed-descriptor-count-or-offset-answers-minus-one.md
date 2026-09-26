# D273 - A failed descriptor, count or offset answers -1

**Status:** assumed
**Date:** 2026-09-26

A call whose result the caller uses directly as a descriptor, byte count or file offset
answers `FAILED_DESCRIPTOR` (-1) on failure, not a `GuestError` placeholder and not a specific
errno.

**Why:** the placeholders stay below the high bit so a trace never mistakes them for firmware
values, which makes them small positive integers - exactly what a valid descriptor or count
looks like. A caller testing `< 0` then reads failure as success. Negative failure is the
POSIX convention on a FreeBSD-derived kernel; which errno comes back is left to a hardware
probe.

**Rejected:**
- The placeholder error code: reads as a valid descriptor, count or offset.
- A chosen errno value: nothing measured says which one the hardware returns.
