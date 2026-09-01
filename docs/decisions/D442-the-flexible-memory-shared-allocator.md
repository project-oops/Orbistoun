# D442 - The flexible-memory "shared allocator" theory was aimed at the wrong titles


**measured** - 2026-09-01 (user-directed, /loop)

The standing theory (carried in the summary going into this session) was that PPSA25872's and
PPSA02664/03416's C++ allocators fail because they size themselves off
`sceKernelAvailableFlexibleMemorySize`, which orbistoun answers from the ~5.25 GiB direct pool instead of
a game's real flexible budget. A static import survey of all four resident titles retires it:

| title | flexible-memory imports |
|---|---|
| PPSA02664 | none - only `sceKernelGetDirectMemorySize` |
| PPSA25872 | none - only `sceKernelGetDirectMemorySize` |
| PPSA03416 | none - only `sceKernelGetDirectMemorySize` |
| PPSA21564 | `sceKernelConfiguredFlexibleMemorySize` **and** `sceKernelAvailableFlexibleMemorySize` |

Three of the four never call a flexible-memory function at all. Their allocators size off
`sceKernelGetDirectMemorySize`, which orbistoun already answers correctly with the measured
`0x1_4000_0000` (D398). So there is no flexible-memory bug for them to fix, and the interim decision to
keep `available_flexible_memory_size` reading the pool (D273, and the comment on
[`FLEXIBLE_MEMORY_SIZE`]) costs those three nothing.

**The one title that does use flexible memory names the real gap: `sceKernelConfiguredFlexibleMemorySize`
is unimplemented.** PPSA21564 imports it (the *configured* total, distinct from *available* = configured
minus mapped; the vendor libkernel exports both, at `0x1a9c0` and `0x1a920`). An unimplemented import
resolves to the `Unimplemented` placeholder, so if the title reaches that call it is told nothing.

**Why this is not yet an implementation.** The honest value of `configured`/`available` for a *game* is
that game's own budget, and a game declares it in its `PT_SCE_PROCPARAM` segment via the kernel
mem-param - not something transferable from obSCEne's homebrew measurement (per-process, the D273
argument) and not a constant orbistoun may invent (principle 3). Reading that structure is a
binary-format concern, which is **SELFish's** remit, not orbistoun's - the proc-param/mem-param layout
belongs in `selfish-elf` beside the dynamic/dynlib tables it already reads, sourced from a citable
open-source implementation. It is deferred here rather than half-built in the wrong crate:

- SELFish is mid-rescaffold (an active session: empty workspace root `Cargo.toml`, a new `selfish-cli`
  being stood up, README rewritten the same hour), so it neither builds nor is mine to edit right now.
- PPSA21564 carries 1555 unresolved imports (the whole libSceAgc GPU surface among them), so whether it
  runs far enough to *reach* a flexible-memory call is unverified - and an unreachable handler cannot be
  trusted (principle 6). The reader is worth building once SELFish is back and once the title is shown to
  reach the call; until then the placeholder is the honest answer.

Recorded so the next session does not re-chase the three-title theory this one falsified.

**Update, same day - the proc-param reader was built, and it falsified this entry's own second premise.**

This entry deferred the proc-param reader to SELFish on the belief that a game declares its flexible
budget in its mem-param. That belief was untested, and testing it needed the reader - so it was built,
as a **reading-half in `orbistoun-elf`** beside the `dynamic`/`reloc` tables that already live there
(`orbistoun-elf::procparam`, `SCE_PROCPARAM`, `Container::proc_param_bytes`; surfaced through
`ProcParamInfo` and `orbistoun-cli inspect`). This partially reverses the "defer, do not half-build"
stance above, on four grounds: the layout is **citable and hardware-confirmed** (obSCEne's `crt.c`, which
launches on real hardware, cites the OpenOrbis PS4 ELF specification and pins the mem-param pointer to
`+0x40` from the D219 fault); orbistoun already owns its ELF readers while SELFish is mid-rescaffold and
unbuildable; the reader changes **no runtime behaviour** (it is inspection only); and without it this
entry's deferral rested on an assumption nobody had checked. It should migrate to `selfish-elf` once that
repository is back, like the other reading halves eventually will.

**The check the reader made possible - and it is decisive.** The three param pointers are stored as
`RELATIVE` relocations (zero in the file, `base + addend` at load), so they must be resolved through the
data relocation table, not read raw. Resolved, every resident title's mem-param block is **present but
empty past its size field** - and obSCEne's own eboot, whose `crt.c` demonstrably fills those pointers,
resolves the same way, which is what confirms the resolution is correct rather than reading zeros:

| title | mem-param vaddr | size | contents past size |
|---|---|---|---|
| PPSA21564 | `0x88dc0e8` | `0x40` | all zero |
| PPSA02664 / 03416 | `0x19240a8` | `0x38` | all zero |
| PPSA25872 | `0x1fe80a8` | `0x40` | all zero |
| PPSA04263 | `0x3b10138` | `0x40` | all zero |
| PPSA28061 | `0x1a80c8` | `0x40` | all zero |
| obSCEne (control) | `0x730378` | `0x38` | all zero |

So **no title on disk declares a flexible-memory budget.** The mem-param-override path this entry pointed
SELFish at does not exist in any of them; every title launches under the *default* budget.

**Which corrects D273 and the `FLEXIBLE_MEMORY_SIZE` comment.** Both say obSCEne's measured `0x1b40_0000`
is a per-process figure that must not transfer because "a game reserves far more" through its own budget.
The evidence above is that no game reserves more - every one carries the same empty mem-param obSCEne
does, so obSCEne measured `sceKernelAvailableFlexibleMemorySize` under *exactly* the condition every title
launches under. `0x1b40_0000` is therefore the **system-default available figure and system-wide**, the
kind that transfers. (obSCEne's flexible round-trip also maps at `~0x2_0037_4000`, a region well clear of
the direct pool, confirming flexible memory is a *separate* space, not a view of the direct pool as D273
models it.)

**Still not wired, for narrower and now-precise reasons:**

- **`available` cannot be a constant.** It is configured-minus-mapped and must fall as the guest maps
  flexible memory, so answering a fixed `0x1b40_0000` is only right at the first call. The correct shape is
  a *separate* flexible budget/allocator (not the shared direct pool of D273), seeded at the measured
  default; `available` reads its free bytes. Separating the pools is also what makes this safe where
  capping the shared pool was not - it cannot touch the direct pool the three non-flexible titles size off,
  which is the likeliest explanation for the recorded "imposing the figure took PPSA02664 backwards"
  result, since PPSA02664 imports no flexible-memory function and cannot be affected by the size query
  itself. That regression should be re-derived against a separated allocator before the model changes.
- **`configured` has no measured value.** obSCEne never calls `sceKernelConfiguredFlexibleMemorySize` -
  it is absent from its census - so the *configured* total (>= available) is unmeasured, and inventing one
  is what principle 3 forbids. The clean unblock is an obSCEne probe for it; until then `configured` stays
  the honest `Unimplemented`.
- **Reachability.** PPSA21564 still carries 1555 unresolved imports, so it has not been shown to reach a
  flexible-memory call. Per principle 6 the handler waits for a title that does.

The reader and its `inspect` output are the durable product of this turn: SDK-version detection for free,
and the oracle any future flexible-memory work confirms a cited mem-param layout against.

**Follow-up: the obSCEne probe named above now exists.** obSCEne D283 adds `020-memory/flexible-configured`,
reading `sceKernelConfiguredFlexibleMemorySize` and reporting the value - the configured default this entry
said was unmeasured. Once a hardware run captures it, orbistoun can implement `sceKernelConfiguredFlexibleMemorySize`
against a measured figure (system-wide, since no title overrides it) rather than the `Unimplemented`
placeholder, and seed the separate flexible allocator scoped above. Still gated on a title reaching the call.

