# D268 - Floating-point arguments never reached an implementation


**decided** · 2026-08-25 · thirteen conformance failures, one cause

`GuestFn` was `fn(&[u64; 6]) -> u64`: six integer registers in, one out. That is the whole
call boundary, and there was no `xmm` anywhere in the dispatch path.

A `double` travels in `xmm0`-`xmm7` and **never** in the integer registers. So the guest put
4.0 in `xmm0`, the handler read six integer registers that did not contain it, and answered
in `rax`, which the guest was not reading. `sqrt(4)` came back as **4** - the guest's own
argument, still sitting in `xmm0` because nothing had written it.

Thirteen checks in one section failed that way. Every floating-point function on the
platform was unreachable, and no title had said so, because a title just faults somewhere
downstream.

The trampoline spills all eight floating-point argument registers and loads `xmm0` from a
slot the handler writes. **Eight stores, unconditionally**, against six pushes and a call
that already happen - the alternative is a second trampoline chosen per import at table
build time, which the stubs could carry because they already hold their own trampoline
address. That is the escape hatch if it ever measures badly; half an ABI is not.

`GuestFloatFn` is a second type rather than a widened first one, because the busiest import
in the corpus is called ninety-nine million times without touching a float. It answers raw
bits rather than an `f64`, so `sqrtf` does not have to lie about its return type.

