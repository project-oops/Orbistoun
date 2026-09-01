# D450 - PPSA02664's "regression" is a thread race between two concurrent walls, not a regression


**measured** - 2026-09-01 (user-directed /loop)

This revises the premise of [D449](D449-ppsa02664-regressed-to-the-allocator.md). D449 treated
PPSA02664 reaching 1541 calls (fault `image+0xb14be3`, `_Getpctype`) as a good state that later
*regressed* to 234 calls (fault `image+0xafcc08`, the tlsf allocator), and built a policy-region/reserve
mechanism to explain the change. **The fault site is non-deterministic across runs of identical code**, so
the two are not before/after states of a regression - they are two outcomes of one thread race.

**The measurement.** Five runs of the *reverted, net-zero* tree, nothing changed between them:

```
run 1: write to 0x0   at image+0xafcc08   (tlsf allocator, ~234 imports)
run 2: write to 0x0   at image+0xafcc08
run 3: write to 0x0   at image+0xafcc08
run 4: write to 0x0   at image+0xafcc08
run 5: read of 0x7fff00cf at image+0xb14be3 (the _Getpctype placeholder, ~1541 imports)
```

The guest is multi-threaded (its boot is a wall of `scePthreadMutex*` init). Two independent faults are in
flight: one thread sets up its tlsf heap early and writes through the null `tlsf_add_pool` returns
(`image+0xafcc08`); another thread runs deeper and eventually dereferences `_Getpctype`'s placeholder
return (`image+0xb14be3`). Whichever faults first is the one reported. The tlsf wall wins roughly four runs
in five, so it is the **early, binding constraint**; the 1541/`_Getpctype` state is the lucky tail where
the tlsf thread was slow. **A single run's fault site is not a stable progress signal for this title**, and
comparing two single runs - which both this turn's first attempts and D449's "regression between 13:41 and
15:15" did - measures the scheduler, not the code.

## What this means for the two walls

- **`image+0xafcc08` (tlsf allocator) is the real frontier.** `tlsf_add_pool: Memory size must be between
  0x28 and 0x100000000 bytes.` prints, the pool is refused, the next allocation returns null, and the guest
  writes through it. This is D449's open question and it is unchanged: the size the guest passes
  `tlsf_add_pool` is outside `[0x28, 0x100000000]`, and its source is still unfound.
- **`_Getpctype` is a real but secondary bug** - the D344/D165 class, a pointer-returning function
  (Dinkumware's `<ctype.h>` accessor; `libSceLibcInternal` is Dinkumware-derived) that falls to the default
  stub and answers the placeholder error code, which the guest dereferences as a table pointer. Fixing it
  removes *only* the `0xb14be3` branch of the race: with `_Getpctype` implemented, every run then hits the
  tlsf wall. It does **not** get past the tlsf wall, so on its own it lowers the best-*recorded* run from a
  lucky 1541 to a consistent 234 without buying real progress. Left **unimplemented, net-zero**, for the
  same reason D449's experiments were reverted: the binding constraint is tlsf, and a correct `_Getpctype`
  needs the Dinkumware class-bit masks (readable off the guest now the base is known, below), not the
  zeroed table a naive fix would invent - a zeroed table is plausible-but-empty output (principle 3).

## Two facts established for the next (focused, non-loop) turn on the tlsf wall

**The load base, so disassembly is finally tractable.** orbistoun maps PPSA02664's first LOAD segment at
`0x400000400000`, but the fault reporter's `image+X` label anchors at `0x400000000000`. So for any
`image+X`, the byte in the eboot's inner ELF is at **file/vaddr offset `X - 0x400000`** (the inner ELF
starts at eboot offset 416; its executable LOAD is at file offset `0x4000`, vaddr base 0). Verified against
a known call site: the `sceKernelMapDirectMemory` return `image+0x1596189` -> offset `0x1196189`
disassembles as coherent GOT-indirect call code (`call [rip+...]` through `0x1a8e000`), while the naive
`image+X == offset` reading disassembles as garbage. So the tlsf wall `image+0xafcc08` is inner-ELF offset
`0x6fcc08`, and D449's "disassemble the `tlsf_add_pool` call site" is now a concrete address, not a
guess. (`llvm-readelf`/`llvm-objdump` choke on the wrapper's section table pointing past the flat file;
zero `e_shoff`/`e_shnum` to read program headers, and use GNU `objdump -D -b binary -m i386:x86-64
--adjust-vma=<offset>` on the extracted executable segment.)

**The tlsf size is not parsed through the ctype table.** Filling `_Getpctype`'s table with all-ones (every
character every class) versus all-zeros left the `tlsf_add_pool` rejection byte-identical. So the bad pool
size is not the output of a `ctype`-driven string parse - that hypothesis is eliminated, alongside D449's
already-eliminated reserve base and `sceKernelVirtualQuery` extent. The size comes from some other value
the guest holds; finding it is a disassembly of the `tlsf_add_pool` caller at offset `0x6fcc08` and back.

The tree is left net-zero (all code reverted; only these notes added). obSCEne is unaffected.
