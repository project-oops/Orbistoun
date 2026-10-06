# D751 - An arena created over a base serves from that range

**Status:** decided
**Date:** 2026-10-06

An mspace a guest creates over a base and capacity (`sceLibcMspaceCreate(name, base, capacity,
flag)` with both non-zero) hands out every block from inside `[base, base + capacity)`, as dlmalloc's
`create_mspace_with_base` does; a full arena answers null. Its bookkeeping is kept on the host, so
nothing is written into the range but what the guest writes, and `sceLibcMspaceMallocUsableSize`
answers the size asked for, as the platform's own heap does. An mspace created without a base stays
on the shared heap (D451).

**Why:** D451 left this case unmodelled until a guest relied on it. PPSA21564 maps 184 MiB the GPU
sees at `0x400000000` (`sceKernelMapNamedDirectMemory`) and builds its Onion graphics heap there as
an mspace (`sceLibcMspaceCreate(0, 0x400000000, 0xb800000, 1)`). Blocks from the shared heap lie
outside that range, its heap refuses them, and the title stops on its own assert: "Out of graphics
memory [Onion]. size = 2097152, used 0/192937984". A block from anywhere but the range is memory the
GPU was never given.

**Rejected:**
- Keeping D451 everywhere: it answers a GPU heap with CPU memory, which is wrong whether or not the
  title checks.
- Running dlmalloc's own arena layout inside the guest's range: the guest reads its blocks only
  through this family, so the layout buys nothing over host-side bookkeeping and writes headers
  into memory the guest owns.
