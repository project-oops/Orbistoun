# 2026-09-01 - (/loop) Map direct memory into an existing reservation; crack image+0xafcc08

With the return column from D459 in hand, cracked the `image+0xafcc08` wall it had just made
legible. The returns showed `sceKernelMapDirectMemory -> 0x7fff0004` (`NoMemory`) while the three
memory calls before it all answered `0x0`: the guest reserves a virtual range and then maps
physical memory inside it, and `map_named_direct_memory` was reserving the range a second time
and conflicting with the reservation already holding it. Added `AddressSpace::owns` (the
containment test `protect` already did, now shared) and made map commit into an owned range with
`protect` instead of reserving afresh; only an un-reserved address is a fresh mapping now.
Recorded D460.

Verified the way this project has to - the wall moved *and* a second, independent number agreed:
`0xafcc08 -> 0xb14be3`, and 234 calls/26 imports became 1544 calls/39 imports (+1310, +13),
verdict FURTHER. A program cannot make thirteen hundred more calls without a working heap, so
this is the allocator initialising, not a wall pushed sideways by a wrong answer. `map` now
answers `0x0` where it answered `0x7fff0004`, read from the same column that found it.

Companion fix, from this session's own earlier work: the kernel's
`every_implementation_is_also_declared_here_or_says_why_not` meta-test failed on
`pthread_key_create`. The thread-specific-data keys (D453) and POSIX unnamed semaphores (D455)
are implemented in `orbistoun-kernel` and declared in the POSIX module - reachable, not dead -
but the test's `DECLARED_ELSEWHERE` exception list was not updated when they were added. Added
the four `pthread_key_*` and five `sem_*` names beside the existing `pthread_create` precedent.
`cargo test` green (mem 77 incl. two new `owns` tests, kernel 33); clippy clean on both crates;
fmt applied.

Next: `_Getpctype` at `image+0xb14be3` - a pointer-returning ctype function whose placeholder the
guest dereferences (D459). It needs a real classification table, taken from an oracle (FreeBSD /
the guest's own indexing) rather than guessed, and the earlier revert of a `_Getpctype` attempt
was the D450 non-determinism misread as a regression, not a real one - so re-approach it with the
return column now telling the truth about what it answered.
