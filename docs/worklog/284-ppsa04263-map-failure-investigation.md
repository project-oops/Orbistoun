# 2026-09-01 - (/loop) PPSA04263's map wall: a rigorous narrowing, not yet a fix

PPSA04263 walls at `image+0x2ba64c1` (write through `r12 = 0`). The return column (D459) traced
it straight to `sceKernelMapDirectMemory -> 0x7fff0004` (`NoMemory`) - the guest reads that as
out-of-memory, keeps the null its allocator returned, and writes through it. So it is the *same
function* as the D460 wall but a **different path**: this title does `AllocateDirectMemory ->
MapDirectMemory` with **no `ReserveVirtualRange` first**, so D460's `owns` branch does not apply and
map falls to `reserve`, which fails.

Dumped the map's arguments (`ORBISTOUN_DUMP`): `len = 0x32000000` (838 MB), `align = 0x200000`
(2 MB), `physical = 0x10000`, and the out-param holds `0`, so it bump-allocates a base. Then spent
the tick ruling out the obvious causes, and the surprise is that **all of them are innocent**:

- **Not the host commit limit.** An isolated `VirtualAlloc(838MB, MEM_RESERVE|MEM_COMMIT)`
  succeeds; the host had 18 GB of free commit.
- **Not the base.** Fixed-base 838 MB commits at `0x720000000000`, `+0x200000` and
  `0x730000000000` all succeed in isolation.
- **Not a `mappings()` overlap.** It is the *only* mapping call in the whole 32-call run - no
  `ReserveVirtualRange`, `mmap` or other map - so the region list is empty and `validate` finds
  nothing to conflict with.
- **Not the direct pool.** `DirectMemory` is physical-offset bookkeeping (a `Vec<Region>`), not a
  host reservation, so it occupies no virtual address. `allocate_direct_memory` correctly returns
  physical `0x10000` (past `RESERVED_LOW`), which matches the map's `physical` argument.
- **Not a large upfront reservation.** The loader/worker reserve the image, data imports, TLS and
  stacks at `0x4000…`/`0x6000…`; nothing carves a big span across `0x7200…`.

So the equivalent `VirtualAlloc` succeeds standalone but the emulator's `reserve` of the same size
at the same arena fails in-process - which means something the emulator reserved is occupying that
address by a path `mappings()` does not track, or the base is not what the empty-arena reasoning
says. I cannot see which, because the failure is invisible twice over: **Windows `platform::reserve`
reports every `VirtualAlloc` null as `Conflict`** regardless of `GetLastError` (the D010 gap the
Unix path already closed - EEXIST/ENOMEM vs the rest), and **`map_named_direct_memory` collapses
every reserve error into `NoMemory`**, so the report never sees the real reason or the real base.

Next tick, before touching the bug: make it legible. (1) Windows `reserve` distinguishes the null
cause via `GetLastError` (address-taken -> `Conflict`; commitment/limit -> `HostRefused` with the
code), the D010 parallel. (2) `map` captures its last reserve failure - base, len, error kind - into
allocation-free statics surfaced in the run report (the D458 pattern; no guest-stack logging, D381).
Then re-run PPSA04263 and read what it actually says. Only then fix it.
