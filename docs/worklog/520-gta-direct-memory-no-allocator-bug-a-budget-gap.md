# 520. GTA V's direct-memory refusal: no allocator bug - a budget gap

**2026-09-12** - task_f446669a: "pin and fix the allocator bug"

Instrumented the refusal (task's next step) and it **falsified the task's premise**. There is no
allocator bug. The pool is not empty when GTA asks; GTA legitimately over-demands it, and the allocator
correctly refuses.

## What the instrumentation showed

A temporary dump of the pool's regions in the `NoMemory` branch of `allocate_main_direct_memory`, on
`./bin/orbistoun run PPSA04263`:

```text
DIRECT alloc refused: len=0x120f00000 align=0x200000 memtype=12
  region 0x0..0x10000        allocated   (RESERVED_LOW)
  region 0x10000..0x32010000 allocated type=12   (~838 MiB)
  region 0x32450000..0xf6050000 allocated type=12   (~3.06 GiB)
  region 0xf6850000..0x101450000 allocated type=12   (~172 MiB)
  region 0x101450000..0x140000000 free               (~1.02 GiB)
```

So ~3.9 GiB of type-12 direct memory is already allocated, and only ~1 GiB is free - the 4.51 GiB
request cannot fit. `ORBISTOUN_DUMP=sceKernelAllocateDirectMemory` shows why: GTA calls
`sceKernelAllocateDirectMemory` **twice before** the failing call - `len=0x32000000` (838 MiB) and
`len=0xc3c00000` (3.06 GiB), both type `0xc` - and there are **no `sceKernelReleaseDirectMemory` calls**.
So GTA's cumulative live demand is ~838 MiB + 3.06 GiB + 4.51 GiB = **~8.4 GiB against a 5 GiB pool**.

The diagnostic was removed; the tree is unchanged (`git diff` on `orbistoun-kernel/src/lib.rs` empty).

## Why there is no allocator bug

`allocate_aligned`/`allocate`/`release` are correct: they refuse when the pool cannot fit a request, and
that is exactly what should happen when ~8.4 GiB is demanded of a 5 GiB pool. The task's "empty pool, so
4.51 GiB must fit" was based on `ORBISTOUN_DUMP=sceKernelAllocateMainDirectMemory` showing only one
call - it hid the two larger `sceKernelAllocateDirectMemory` allocations that fill the pool first.

## The real gap: the pool is obSCEne's budget, not a retail game's

`DIRECT_MEMORY_SIZE` = 5 GiB is measured - but measured from **obSCEne's conformance probe**
(`sceKernelGetDirectMemorySize` under its own process, D398/D442). GTA V is a retail AAA title and
demands ~8.4 GiB of direct memory. A guest that sized its heaps off the query would have stopped at
5 GiB; GTA does not - it asks for 8.4 GiB regardless, so its budget expectation is larger than obSCEne's
probe budget. orbistoun's single global 5 GiB pool is therefore too small for GTA, and there is no
per-title budget mechanism (GTA's `param.json` carries only a 512-GiB VA range, no direct-memory
budget; its compat `[settings]` is empty).

This is not the 2026-09-07 -> now regression it looks like either. That record (`image+0x196b91a`,
30,261 calls) was taken under the **positive** placeholder (pre-D670), where GTA read unimplemented
calls as success and wandered further down a wrong path. Post-D670 (negative placeholder) GTA takes a
more honest path and reaches its real early wall - this allocation - at 20,469 calls. The frontier
number fell for the same reason it fell corpus-wide: the lie stopped inflating it.

## What would actually move GTA

Not an allocator change. Either:
- a **per-title direct-memory budget** (a compat `[settings]` override sizing the pool per title), fed by
  a real value - which needs a measured retail-game direct-memory budget, not obSCEne's probe's 5 GiB; or
- confirmation that a retail title's direct budget on this console genuinely exceeds 5 GiB (open: obSCEne
  measures its own process at 5 GiB and cannot directly measure GTA's; whether 5 GiB is the probe's
  budget or the system max for any title is the question).

Until that value exists, sizing the pool up would be guessing, and 5 GiB stays the honest measured
default. Recorded as "no bug; budget gap" rather than a fix, so the allocator is not "corrected" into
wrongness.

## State

- No code change (instrumentation added and removed). Suite unaffected.
- task_f446669a resolves as: premise falsified, no allocator bug; the blocker is the pool-vs-budget gap
  above, which is a data/design question, not a defect.
