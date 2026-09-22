# 782. orbistoun applies all of ASTRO BOT's relocations including the arena-init pointer, disproving the RELRO-loader hypothesis; the wall is the same indirect-caller obstacle as the others, so the grind pauses

**2026-09-21** — worklog 781 read ASTRO BOT (PPSA21564)'s arena-init pointer as a RELATIVE reloc into a
RELRO segment and, on a failed `PEEK`, proposed that orbistoun does not apply it - a loader bug. The
measurement refutes that. It is worth recording that the loader is *not* the fault before the next
session chases it, and where that leaves the wall.

## The relocation is applied

orbistoun's own loader summary for this title:

```text
placed 5 segments (251341377 copied, 11687808 zeroed) at 0x400000000000;
relocations 560259/560259 applied (0 weak-zero, 0 TLS-deferred, 0 unsupported, 0 unresolved)
```

Five `PT_LOAD`s placed (ASTRO BOT has exactly five, `ph[0,1,3,7,8]`), and **560259 of 560259**
relocations applied with **zero** unsupported or unresolved - the RELA table is ~558.5k entries plus the
`JMPREL` set, so this is the whole thing. The `R_X86_64_RELATIVE` that writes `base + 0x27cc20` into
`image+0x88dc158` is one of them and is applied. So `ph[3]` is mapped and the arena-init's method-table
pointer *is* populated. The `PEEK` that read `0x88dc150` as "outside every readable span" was `PEEK`'s
published-span list not including a RELRO page, not the page being absent - a limitation of the
diagnostic, and a correction to 781.

(The "57971 applied" figure 781's suspicion leaned on was a **Terminator** run mis-remembered as this
one; ASTRO BOT's count is 560259, matching its table. Noted so the false lead is not re-derived.)

## Where the wall actually is

With the pointer correctly placed, the arena-init `0x27cc20` would run if anything called through
`image+0x88dc158`. It does not (778: its first write never happens). And the method table at
`0x88dc158` has **0 direct code references** (this session's scan) - it is reached indirectly, through a
pointer-to-the-table held in some global that static search does not connect. That is the identical
obstacle GTA's registration and Terminator's pool hit: the invocation is one indirection past what a
`lea`/`call`-target scan resolves, and following it needs a tool (resolve which relocated global holds
`&table` and who reads it), not another disassembly window.

So ASTRO BOT joins the others at a sustained-effort wall - and importantly, **not** at a loader bug: the
relocations, RELRO mapping and arena-init pointer are all correct. The remaining gap is why the startup
path that should call the arena's create-method does not run under orbistoun, which is the same
un-run-startup question as PPSA04263, now with the confirmation that relocation is not the cause.

## Gate state

No code changed - a correction that measures all 560259 relocations applied (disproving 781's RELRO-loader
hypothesis and retracting the mis-attributed 57971 figure), and records ASTRO BOT's wall as the same
indirect-caller obstacle as the other titles, not a loader fault. `./bin/orbistoun check` green, worklog
index regenerated, identity scan clean. No commit.
