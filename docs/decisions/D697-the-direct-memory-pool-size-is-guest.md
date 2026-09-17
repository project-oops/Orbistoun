# D697 - The direct-memory pool size is guest-dependent, defaulting to the retail figure

**assumed** - 2026-09-15

`sceKernelGetDirectMemorySize` and the pool a guest allocates from are now driven by
`Settings::pool_bytes`, which defaults to twelve gibibytes and can be set to five. Both are hardware
measurements; the reason there are two is the reason this is a setting.

## Two measurements, not a correction

D398 measured this call at five gibibytes on a conformance/homebrew run and made it the constant,
replacing an assumed eight. `REQ-...5d1c` then measured it at **twelve** gibibytes on a retail eboot
leg (verified in `20260915-125124-eboot.obs.log`: `020-memory/direct-size|pass|0x300000000`). Both
are real, and they do not contradict each other: the platform's sandbox hands a retail title and a
homebrew payload different budgets, the same way it hands them different filesystem and network
reach. A homebrew payload getting less direct memory than a retail game is the expected shape, not
an anomaly to reconcile.

## Why the retail figure is the default

The corpus is retail, and every direct-memory wall in it is retail. PPSA04263 asks for 4.51 GiB
after taking ~4 GiB - 8.5 GiB of demand that a five-gibibyte pool refuses with `NoMemory` and a
twelve-gibibyte pool answers. At 12 GiB the title reaches its recorded position (70 imports,
`image+0x196b91a`) instead of dying in the allocator. The default that makes the primary corpus
correct is the retail one.

A homebrew or conformance leg (obSCEne, the payloads) sets `pool_bytes` to
`HOMEBREW_DIRECT_MEMORY_SIZE` through its config. No homebrew guest in the corpus allocates near
either ceiling, so the divergence - obSCEne under orbistoun currently seeing 12 GiB where hardware
gave it 5 - changes a reported number and nothing a guest does. It is a correctness refinement a
homebrew leg opts into, not a wall.

## Why one setting for both the report and the pool

D398 established that a guest reads `sceKernelGetDirectMemorySize` and walks the map against it:
change the size and the guest's next query moves, exactly tracking. So the number the query reports
and the size of the pool it hands out must be identical, or a guest sizes its heaps against memory
the walk will never show it. Reading both from one `pool_bytes` field makes that structural - there
is no second constant to drift.

## The alternative, and why not now

Detecting a guest's sandbox privilege automatically - retail vs homebrew - and selecting 5 or 12
without a config line. Rejected as premature: the signal (which sandbox a guest runs under) is not
currently modelled, no corpus guest is mis-served by the retail default, and building the detection
before anything needs it is the speculation principle 12 warns against. The setting is the seam; the
automatic selection is a later fill-in when a homebrew guest that allocates hard appears.

## What would overturn this

A third measurement - a different console model or firmware answering something other than 5 or 12 -
which would make `pool_bytes` a per-machine value rather than a per-guest-class one. The setting
absorbs that without a code change; only the default would be reconsidered.
