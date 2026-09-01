# 2026-09-01 - proc-param reader built; the mem-param oracle falsified the budget premise


Followed the D442 diagnosis with the reader it was waiting on, then let the reader settle the question
D442 had only assumed. Built `orbistoun-elf::procparam` (a `ProcParam` header parser), `SCE_PROCPARAM`
(0x6100_0001), and `Container::proc_param_bytes`, surfaced through a new `ProcParamInfo` on `ContainerInfo`
and printed by `orbistoun-cli inspect`. Layout is citable and hardware-confirmed - obSCEne's `crt.c` +
the OpenOrbis PS4 ELF spec, mem-param pointer at `+0x40` from the D219 fault (REFERENCES.md, new section).
Built it in orbistoun-elf as a reading-half beside `dynamic`/`reloc` rather than in SELFish (mid-rescaffold,
unbuildable), reversing D442's "defer" on the grounds that answering the oracle needed it and it changes no
runtime behaviour; to migrate to `selfish-elf` later.

**Surprise, and the reason to have a control.** The three param pointers first read as all-zero on every
title - including obSCEne's own eboot, whose `crt.c` provably fills them. That control exposed the cause:
the pointers are `RELATIVE` relocations (zero in the file, `base + addend` at load), so they must be
resolved through the data relocation table. With that done (`relative_relocation_targets`), every resident
title resolves to a real mem-param block that is **present but empty past its size field** - obSCEne's too.
So no title on disk declares a flexible-memory budget; the mem-param-override path D442 pointed SELFish at
does not exist. obSCEne's measured `0x1b40_0000` was taken under that same empty-mem-param condition, so it
is the system default (correcting the D273 / `FLEXIBLE_MEMORY_SIZE` "per-process, does not transfer" claim -
both comments updated). The runtime still is not wired: `available` cannot be a constant (it falls as the
guest maps), `configured` has no measured value (obSCEne never probes it), and PPSA21564 has not been shown
to reach the call - all recorded in the D442 update with the scoped next step (a separate flexible allocator
seeded at the default). Tests pass (elf/proto/service), clippy clean on touched crates; the reader ships
tested with an obSCEne-shaped fixture.

