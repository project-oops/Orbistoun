# D223 - Three cheap diagnostics, and a hypothesis nobody had raised


**decided** · 2026-08-24 · directed by the user: *"get as much info as possible from as many
sources as it can, and reduce the debug loop"*

Three more diagnostics, chosen because each answers a question the existing ones could not
and none needs platform code: `ORBISTOUN_BSS_FILL`, `ORBISTOUN_WATCH`, `ORBISTOUN_POKE`.

### The snapshot beat the watchpoint, and that reordering was the point

The plan had been a hardware watchpoint next. A **snapshot diff** turned out to answer the
live question better and for a fraction of the cost: copy a region before the guest runs,
compare afterwards, report which words changed.

A watchpoint says *which byte, when, and from which instruction*, and costs a debug
register, a per-platform API and an exception per access. A snapshot says *which bytes
ended up different*, for one memcpy. For **"did anything ever fill this slot in?"** the
second is the whole answer and the first is a more expensive route to it.

It immediately produced the shape of the object the wall faults on:

```
0x4000019e9ca0  0 -> 0x00004000019765c8   an image pointer
0x4000019e9ca8  0 -> 0x0000000000004000   16 KiB
0x4000019e9cb0  0 -> (never written)
0x4000019e9cb8  0 -> 0x000001d3dd792bd0   a host heap address, different every run
```

**Correcting an earlier claim of mine:** I had said the watchpoint would be Windows-specific.
The debug registers are an **x86** feature - same silicon and semantics on Linux, only the
API to set them differs - and this repository already `#[cfg]`-splits exactly that shape in
`platform.rs`, `fault.rs` and `report.rs`. It is ordinary here, not exceptional, and saying
otherwise made the option sound worse than it is.

### `POKE`, and the seventh elimination

The unwritten slot at `+0x10` was the obvious candidate for the zero base. `ORBISTOUN_POKE`
writes a value at an absolute guest address after relocation and before entry - the
absolute-address counterpart to `ORBISTOUN_WRITE`, which can only reach what an argument
points at.

Poked with two different values: **the fault did not move.** And the watch confirms the
poke survived untouched to the fault, so this is an elimination rather than a value that
got overwritten - a distinction worth checking, because the two look identical from the
fault address alone.

### What is left, and it is not what anyone was looking for

Seven things eliminated at `image+0xafc959`: the stub return, unwritten stack, unwritten
heap, `arg0`'s target, `arg5`'s target, `memalign`, and now the object's unfilled slot.

Which makes the surviving reading a different one entirely. `rcx = 0xfffe0` is
`0x100000 - 0x20` and `rdx = 0x100000`, and every candidate for a *base* that got lost is
gone - so perhaps there is no lost base. **Perhaps `0xfffe0` is a legitimate address that
the real machine maps and this one does not.** A guest writing near the top of a fixed
low-memory region would look exactly like this, and the run already says the honest thing:
*an address in no region this run mapped*.

Cheap to test, and the address space is ours: map a page there and see whether the guest
proceeds. Recorded as a hypothesis rather than a finding - it has not been tried.

### `BSS_FILL` works and is too blunt for this

Filling 1.25 MB of static data with `0xa5` **moved the fault** - to a different instruction,
much earlier. So the guest does read uninitialised statics, and poisoning all of them breaks
too much to isolate one. It stays because the answer it gives is real; it is simply the
wrong instrument for a single slot, which is what `WATCH` and `POKE` are for.

