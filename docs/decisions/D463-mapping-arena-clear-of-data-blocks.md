# D463 - The guest mapping arena is moved clear of the thunk data blocks


**measured** - 2026-09-01 (user-directed /loop: overnight oracle-free crunch)

`MAPPING_BASE` - where a guest map goes when it expresses no address preference - was
`0x0000_7200_0000_0000`, which is `orbistoun_thunk::SUGGESTED_DATA_BASE` **exactly**. The thunk
data blocks are reserved at that address when the guest is loaded, so the first guest map with no
hint (`next_mapping_base` starts the counter at `MAPPING_BASE`) landed on top of them:
`VirtualAlloc` refused the address, `reserve` returned a conflict, and `map` answered `NoMemory` -
which the guest read as out-of-memory and faulted through the null it kept. PPSA04263 died this
way at `image+0x2ba64c1` after 32 calls, and the collision was invisible until D462's diagnostic
named the base.

A title that reserves *with a hint* first - PPSA02664 calls `sceKernelReserveVirtualRange` at its
own `0x5000…` address - never reached `MAPPING_BASE`, which is why the same allocator wall looked
title-specific rather than structural. It is not: any guest that maps without a hint hits it.

`MAPPING_BASE` moved to `0x0000_7400_0000_0000`, a clear terabyte above the data blocks. Both
arenas keep multiple terabytes below the `0x7FFF…` user-space ceiling, and the mapping arena is
now the only thing addressed from `0x7400`, so a stray pointer into it is still recognisable by
its address alone (the property the constant's comment always claimed and did not have).

**Verified, two observations.** The reservation-failure line disappeared *and* PPSA04263 went from
32 calls to 10,123 (`+10,091`), faulting far later at `image+0x2ba47bc`; a program cannot make ten
thousand more calls without the map that failed now succeeding. Blast radius: PPSA25872 advanced
`26,850 -> 39,929` calls (`+13,079`) as well, and PPSA21564 and PPSA02664 were unchanged (no
regression). The now-visible `0x6b0000000000` conflicts on other titles are the guest's own hint
address being refused and its allocator falling back to the arena as designed - non-fatal, and
surfaced only because D462 now reports every reservation failure, not because this introduced one.
