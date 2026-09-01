# D124 - The biggest wall was C++ static initialisation

**decided** · 2026-08-20

`libc::0x8fcfa7779ac4cbb6` was 53.5% of every import call across every title - more than
everything else combined - and unnamed. It is **`__cxa_atexit`**.

Found by reasoning about the *shape* of the traffic rather than by widening the search: a
libc function called more often than all others together is either allocation or static
initialisation, both of which are C++ runtime symbols, and no C or POSIX word list
produces a mangled ABI name. Adding the Itanium C++ ABI - a published specification -
named it, along with `__cxa_guard_acquire` and `__cxa_guard_release`.

1,201 calls in one title is not a loop. It is 1,201 global objects registering
destructors, which is what a game does.

**The guard pair was worse unimplemented than absent.** `__cxa_guard_acquire` returns
non-zero to mean "not yet initialised, go ahead". An unimplemented version returns an
error, which *is* non-zero, so the guest initialised - then called `release`, which did
nothing, so the flag never set, so the next visit initialised again. Every function-local
static reconstructing on every visit, forever.

Implementing the three cost the titles a few hundred calls of pointless work and moved
none of them further, which is itself informative: they were getting through static
initialisation and failing somewhere downstream.

