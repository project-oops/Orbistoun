# D437 - The hardware memory data was already captured; the map does not start at zero


**measured** - 2026-09-01 (user-directed, /loop)

The remaining walls (D436) needed the memory subsystem measured against hardware - and obSCEne had
already captured it. `reports/hardware/console-report-klog.txt` and `ps5-full-run.txt` carry a full
`020-memory` cycle from a real console:

- `sceKernelGetDirectMemorySize` = `0x1_4000_0000` (orbistoun already matches, D398).
- `sceKernelAllocateDirectMemory` from a clean state = **`0x10000`** (a later call, `0xff0000`) - the
  platform reserves the first `0x10000` of the direct range and **never hands a guest physical zero**.
- `sceKernelMapDirectMemory` = `0x2013_60000` / `0x2003_74000`; `sceKernelMapFlexibleMemory` =
  `0x2003_78000` - direct maps land in the `0x2_0000_0000` region, not orbistoun's `0x7200…`.
- `sceKernelAvailableFlexibleMemorySize` = `0x1b40_0000`; `Munmap(null)` = `0x80020016`.

**Applied: the map default is now `ReservedLow`, with the floor measured.** D083 and D218 recorded that
the one variable never swept was "a single free region starting at zero", and predicted `ReservedLow`
was what a guest rejecting offset zero would need. The measurement settles it: the map does not start at
zero, and the floor is `0x10000` (was an arbitrary 512 MiB). PPSA02664 went **FURTHER** on it. The other
walls held, so this is the honest baseline, not their fix.

**Not applied, and why it matters: the flexible budget.** `0x1b40_0000` is the *probe's* budget, and the
flexible budget is set per process - a game reserves far more. Imposing the probe's figure took PPSA02664
backwards, because it failed maps the game makes within its own budget. This is the "a measurement stays
with whatever measured it" rule with teeth: `DIRECT_MEMORY_SIZE` and the `ReservedLow` floor are
system-wide and transfer; a per-process budget does not. Recorded as a reference constant, left unused,
and `AvailableFlexibleMemorySize` keeps reading the pool until a *game's* budget is measured. The direct
map base (`0x2_0000_0000`) is system-wide and does transfer - the next thing to apply, pending its own
test.

