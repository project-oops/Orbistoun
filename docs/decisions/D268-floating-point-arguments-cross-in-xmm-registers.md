# D268 - Floating-point arguments cross the trampoline in xmm registers

**Status:** decided
**Date:** 2026-08-25

The trampoline spills all eight floating-point argument registers on every call and loads
`xmm0` from a slot the handler writes. A function answering in `xmm0` is a separate
`GuestFloatFn` type that returns raw bits.

**Why:** a System V `double` travels only in `xmm0`-`xmm7`, so an integer-only boundary makes
every floating-point function unreachable and returns the guest's own argument. Eight
unconditional stores are cheap beside the pushes and call already made. A separate type keeps
the integer path, which carries the busiest imports, untouched, and raw bits let single- and
double-precision functions state their true return type.

**Rejected:**
- A second trampoline chosen per import: kept in reserve if the stores ever measure badly.
- Widening `GuestFn` to carry floats: every integer call pays for it.
