# 867. PPSA25872's trap traced to the APR resolve's unfilled size

**2026-09-25**. PPSA25872 moved FURTHER on the new build, to its own `int 0x41` at `image+0x17554a3`.
The trap follows a failed `sceKernelReserveVirtualRange` (`0x8002000d`). That was the 35th
reservation, and its length was `0x600000800000`, exactly the guest stack's top.

**New diagnostic: caller stacks.** An import named in `ORBISTOUN_DUMP` now also keeps its caller's
stack, 512 words up from the return address, for its four most recent calls. The report lists the
words that land in guest code. Argument dumps keep an import's *first* calls; the bad call here was
the last. There is no frame-pointer walk, so a stale word can qualify, and the line says so.

**The chain, from the trap up.**

| return address | what it is |
|---|---|
| `0x1ae25f2` | a page-count wrapper around the reservation: `count × 0x4000` |
| `0x7b6c31` | the allocator's large-block path: `count = roundup(size, page) / page` |
| `0x7b4894` | the allocator core: the request rounded up to 256 KiB |
| `0x7b62dd` | the allocator's entry |
| `0x49421e` | `string::reserve(n)`, allocating `n + 1` |
| `0xbedb0c` | the caller of the reserve |

At `0xbedad3` the caller calls a stream's length method into a local, checks it only against `-1`,
and reserves that many bytes. The call just before it is
`sceKernelAprResolveFilepathsToIdsAndFileSizes` for `/app0/Media/RuntimeInitializeOnLoads.json`,
which orbistoun answers with the placeholder and leaves unfilled (D592). The size slot therefore
holds a stack address, `0x6000007f....`, and 256 KiB rounding makes that `0x600000800000`.

**D679 corrected.** Its fill experiment wrote four bytes per slot. The slot at `…fb880` has room
for eight, so the old stack address's upper half survived, and all six permutations failed alike.
Its conclusion that APR was not in the chain is wrong, and its decision not to ship a guess stands.

**Filed:** REQ-20260925T1834Z-a7e2, a signature-fill run for the three out-slots (their order and
widths). PPSA25872 waits on it.
