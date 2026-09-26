# D009 - Placeholder error codes avoid the high bit

**Status:** decided
**Date:** 2026-08-19

`GuestError` placeholders never set the high bit, which real platform error codes set.

**Why:** a placeholder leaking into guest-visible behaviour must be recognisable in a trace and
in a fault address, not plausible. A placeholder used as a pointer faults at an address that
names the placeholder.

**Rejected:**
- Real-looking error codes: a leaked stub reads as a genuine platform failure.
