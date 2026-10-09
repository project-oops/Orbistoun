# D772 - A title's flexible memory setting moves its direct pool

**Status:** decided
**Date:** 2026-10-08
**known_by:** measured (REQ-20261008T1845Z-fm03, `reports/hardware/20261009-104652-eboot.obs.log`: a package stating `flexibleMemorySize` 343932928 reported configured flexible `0x14800000`, available `0x13c00000` and direct size `0x307800000`, the three figures this decision predicts; its direct pool held 0x30 allocations of 256 MiB)

When a title's `sce_sys/param.json` states `kernel.flexibleMemorySize`, that is its configured
flexible memory: what `sceKernelConfiguredFlexibleMemorySize` reports. The available figure is
that, less the share the system maps at launch. The difference from the default goes to the direct
pool or comes from it, so direct and flexible keep the default total: 12 GiB plus 448 MiB, the
measured pair (D697, D444). The worker applies the setting before anything builds the direct map.

**Why:** PPSA21564 states 328 MiB of flexible memory, 120 MiB under the default. It allocates its
direct memory in seven requests totalling about 12,388 MiB, 100 MiB past the 12 GiB orbistoun
offered. Its seventh request failed and it stopped on its own assertion,
"sceKernelAllocateDirectMemory error". With the 120 MiB moved, every request fits and it goes on.
PPSA28061 states 1 GiB, which takes 576 MiB from its pool.

**Assumed:** that the console keeps direct plus flexible constant, so a title's flexible setting
is paid for from its direct memory. The default pair is measured; obSCEne is asked to measure a
title-stated setting. Neither figure moves for a title that states nothing.
