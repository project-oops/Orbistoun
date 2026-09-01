# D444 - obSCEne is orbistoun's oracle now, and it measured the flexible-memory bug exactly


**measured** - 2026-09-01 (user-directed: "obscene works on real hardware so this means there's an issue with orbistoun")

obSCEne runs to completion on real hardware, so its crash *under orbistoun* is orbistoun's bug - and it
exercises 555 checks whose hardware answers are recorded, making it a value-by-value oracle. Two things
were in the way and both are now understood.

**The stale eboot, first.** `titles/obscene/eboot.bin` was a build from the day before (`meta 39/544`),
and it crashed early: obSCEne's `obs_linkmap_own_dynamic` finds its own module base by scanning backward
for the ELF magic, guarded by `sceKernelVirtualQuery` to stop at the first unmapped page - but that stale
build did not import `sceKernelVirtualQuery`, so `obs_scan_page_ok` took its "query not callable → proceed"
branch and walked one page below the base into unmapped memory. Not primarily an orbistoun fault: a vendor
module's ELF header is legitimately not resident at the base (orbistoun matches a packaged title there), and
a *current* obSCEne imports the query. selfish is buildable again (the user deployed obSCEne to hardware
today, which needs its tool), so a fresh `make module` was built and installed as the title. It now runs
**338 checks** under orbistoun before a later wall.

**What the oracle then measured - the flexible-memory bug, exactly.** With obSCEne running, its
`020-memory/flexible-*` checks diffed against the hardware report (`console-klog.01092026.txt`):

| check | orbistoun before | hardware | orbistoun after |
|---|---|---|---|
| flexible-available | `0x1_3f01_0000` (~5 GiB, the direct pool) | `0x1b40_0000` | `0x1b40_0000` |
| flexible-configured | FAIL (unimplemented placeholder) | `0x1c00_0000` | `0x1c00_0000` |

So the "left reading the pool" state (D442/D443) was answering an order of magnitude high, and *configured*
was a gap. Both are now fixed against **measured** figures. Flexible memory is modelled as a **separate
budget** from the direct pool (`orbistoun_kernel::direct`: `FLEXIBLE_CONFIGURED = 0x1c00_0000`,
`FLEXIBLE_MEMORY_SIZE = 0x1b40_0000` as the launch figure, an `AtomicU64` of what the guest has mapped).
`sceKernelConfiguredFlexibleMemorySize` is implemented (was unimplemented); `available` reads the budget
minus mapped, not the direct pool; `map`/`release` charge and credit it and `map` refuses past the budget.
Both figures are the **system default** and transfer, because no title overrides the mem-param (D442). The
obSCEne probe that supplied `configured` is D283, and its positive dlsym probe validated on the same
hardware run (`dlsym-resolves-known-symbol → 0x800968e20`).

This is the loop the two projects were built to run: a probe that passes on hardware, run under the
emulator, names the emulator's divergence in one line and confirms the fix in the next. Verified: obSCEne
under orbistoun now answers both checks byte-exact against the console; a unit test pins the budget
arithmetic; tests/clippy/fmt clean.

**The next wall.** obSCEne gets 338 checks in, then faults at `image+0x4502ff` reading `0x600000801000` -
a stack address one page past `stack+0x800000`. A later probe walks off the guest stack; that is the next
thing the oracle is pointing at.

