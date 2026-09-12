# 518. Testing titles: GTA V's direct-memory anomaly and ASTRO BOT's bundled-module fault

**2026-09-12** - operator: "continue testing titles and iterating the wall"

Tested the two retail titles not yet characterised this session. Both fault; one of the two walls is a
concrete, tractable allocator anomaly worth a focused fix.

## GTA V (PPSA04263): a 4.51 GiB direct allocation fails an apparently-fresh 5 GiB pool

49 imports / 20,469 calls, then a software `int` (GP fault) at `image+0x2bfab2f`, four bytes after
`sceKernelAllocateMainDirectMemory(0x120f00000) -> 0xf7ff0004` (`GuestError::NoMemory`). So GTA asks
for **4.51 GiB** of main direct memory, is refused, and asserts.

Investigated the refusal, because it looks wrong:

- The pool is **5 GiB and hardware-correct** - obSCEne's `020-memory/direct-size` measured
  `sceKernelGetDirectMemorySize` = `0x140000000`, exactly `DIRECT_MEMORY_SIZE` (libkernel.toml). Not a
  sizing gap.
- `ORBISTOUN_DUMP=sceKernelAllocateMainDirectMemory` shows this is GTA's **only** direct allocation
  (arg0 `0x120f00000` len, arg1 `0x200000` = 2 MiB align, arg2 `0xc` type). So the pool is empty when
  it is asked.
- The default pool is `ReservedLow`: free region `[0x10000, 0x140000000)` (~4.9994 GiB contiguous).
  Reading `allocate_main_direct_memory` -> `allocate_aligned` -> `allocate`: the 2 MiB-aligned start is
  `0x200000`, and `0x200000 + 0x120f00000 = 0x121100000 <= 0x140000000`, so it **should** fit and
  return `Some`. Every function in the path is pure accounting; none checks host backing.

So static analysis says the allocation should succeed, and the runtime returns `NoMemory`. GTA runs on
a real console (same 5 GiB), and it asserts immediately (no retry), so on hardware this call must
succeed - which makes orbistoun's refusal a **bug or a hidden pool-state divergence**, not a real
out-of-memory. It is tractable (orbistoun-side, no obSCEne data, no design reversal) and worth fixing:
it would unblock GTA V's startup.

**The next step is instrumentation, not more reading**: dump `guard`'s regions in the `NoMemory` branch
of `allocate_main_direct_memory` (crates/orbistoun-kernel/src/lib.rs ~1121) and re-run PPSA04263 - the
region list at the failure will show whether the pool is unexpectedly consumed/fragmented, or whether
`allocate_aligned` has a large-request bug the unit tests never exercised (their sizes are small).
Handed off as a spawned task.

## ASTRO BOT (PPSA21564): a null+0x38 deref in a bundled module

57 imports / 500,260 calls (it gets far), then `read of 0x38` at `the title's own modules+0x7af792` -
a null-plus-offset deref in a separately-loaded module (like Earthion's game.bin), reached through a
deep chain from the main image. This is the same shape as PPSA02664's render-state fault (an engine object
whose sub-pointer was never populated), in a different module - i.e. the AGC/GPU object-model layer
again, Phase-6 territory. Not pursued further; recorded as a fifth retail title converging on the GPU
engine, distinct only in which module and offset.

## The retail frontier, after this session

- **Earthion**: mapper must-succeed gate (Phase-6 GPU context) - D677, worklog 516.
- **PPSA02664 + Summer Sports**: AGC render-state object model (Phase-6 GPU) - worklog 517.
- **ASTRO BOT**: bundled-module null-deref (Phase-6 GPU object model), 500k calls in.
- **Terminator**: APR resolve - the D592 slot layout is unmeasured *and* its dump lacks
  `ampr_emu.index`; movable via a disk-fallback + the sanctioned `ORBISTOUN_APR_ANSWER` experiment,
  which softens D587 and awaits the operator's go-ahead.
- **GTA V**: the direct-memory allocator anomaly above - the one wall that is a plain bug rather than
  the GPU engine or a design decision.

Four of five retail titles converge on the Phase-6 AGC->Vulkan engine; GTA V and Terminator are the two
whose walls sit below it and are individually movable.

## State

- No code change this unit (measurement + reading). Suite still green from worklog 517.
- GTA V allocator bug handed off as a spawned task with the narrowing above.
