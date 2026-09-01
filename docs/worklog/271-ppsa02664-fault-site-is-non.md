# 2026-09-01 - (/loop) PPSA02664's fault site is non-deterministic; the "regression" was a thread race

Went to chase D449's PPSA02664 allocator wall and found the premise was off. **The fault site varies
run-to-run on identical code:** five runs of the reverted tree gave `image+0xafcc08` (tlsf allocator, ~234
imports) four times and `image+0xb14be3` (`_Getpctype` placeholder deref, ~1541 imports) once. The guest is
multi-threaded and the two faults are concurrent, so which one is reported is a scheduler outcome, not a
code state. That means D443's "1541" and D449's "234" are two faces of one race, and D449's
policy-region/reserve "regression" mechanism was explaining scheduler noise. Recorded as D450.

Two things banked for the focused (non-loop) turn D449 asked for:

- **The load base is resolved**, so guest disassembly finally works. orbistoun maps the first LOAD at
  `0x400000400000` but labels faults `image+X` from `0x400000000000`, so the inner-ELF offset is
  `X - 0x400000` (verified against the `sceKernelMapDirectMemory` call site). The tlsf wall is offset
  `0x6fcc08`. Method: zero the wrapper's `e_shoff`/`e_shnum` (its section table points past the flat file
  and stops `llvm-readelf`), then GNU `objdump -D -b binary -m i386:x86-64 --adjust-vma=` on the extracted
  executable LOAD.
- **The tlsf pool size is not ctype-parsed** - an all-ones vs all-zeros `_Getpctype` table left the
  `tlsf_add_pool` rejection byte-identical. Joins the reserve base and the virtual-query extent as
  eliminated sources; the size still needs the `tlsf_add_pool` caller disassembled at `0x6fcc08`.

`_Getpctype`/`_init_env`/`malloc_stats_fast` are undeclared pointer/CRT-startup stubs (D344/D165 class) and
were implemented and then reverted: `_Getpctype`'s only effect is to remove the `0xb14be3` race branch (all
runs then hit tlsf) without passing the tlsf wall, and a correct one needs the guest's Dinkumware class-bit
masks rather than a zeroed table. Tree left net-zero; obSCEne unaffected.
