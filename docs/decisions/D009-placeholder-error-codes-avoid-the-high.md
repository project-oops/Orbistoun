# D009 - Placeholder error codes avoid the high bit

**decided** · 2026-08-19

Real codes for this platform set the high bit. `GuestError`'s placeholders
deliberately do not, so an unimplemented stub leaking into guest-visible behaviour
is obvious in a trace rather than plausible.

