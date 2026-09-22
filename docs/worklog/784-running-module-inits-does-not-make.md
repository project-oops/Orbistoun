# 784. Running module inits does not make ASTRO BOT's arena-init run, disproving the crt-init hypothesis; the retail deep-RE phase is exhausted and the loop shifts to a heartbeat pending a tool-build or steer

**2026-09-21** — worklog 783 pivoted from title disassembly to the orbistoun side, on the idea that a
placed module's un-run `DT_INIT_ARRAY` (libc's crt init) might be why startup constructions never run.
The test disproves it, which closes the last cheap lead and settles the shape of the whole retail
frontier. This records that honestly and changes the loop's cadence to match.

## The crt-init hypothesis is disproven

`ORBISTOUN_START_MODULES=all` runs every placed module's initialisers, including libc's. Under it, with a
write-watchpoint on ASTRO BOT (PPSA21564)'s arena-manager global:

- `image+0xe553bd0` is still **`never touched`** - the arena-init `0x27cc20` still does not run.
- The run goes **BACK** (faults earlier, at `the title's own modules+0x7a60f4`, 190 calls) - running the
  module inits eagerly hits a module stub sooner, it does not help startup.

So the arena-init is not gated on a module's init. It is in the eboot's own startup, invoked through the
computed-address path 783 could not trace, and no orbistoun startup knob reaches it.

## The retail frontier, settled

After ~20 ticks the six retail titles are exhaustively characterised, and every one ends at the same
wall with every cheap avenue tried and recorded:

- **Static RE** to a fixed reference: blocked - the invocations are computed addresses (native) or
  metadata-table indices (Il2CPP).
- **Runtime watchpoints**: confirm the construction never runs, but cannot show the un-taken call.
- **The reloc graph** (built this session): follows one indirection level, then hits computed addressing.
- **Relocation/RELRO handling**: correct (ASTRO BOT applies all 560259 relocations).
- **Module inits**: do not drive the missing eboot constructions (this worklog).

What is left is not a one-tick step: an **execution trace from entry** that shows where orbistoun's
startup diverges from calling these constructors, or a **differential against a reference emulator** that
boots them. Both are builds. The reloc-graph query is the first such tool and stays; it is not enough
alone.

## Cadence

Because no further loop-cadence probe advances any title, the loop moves to a **low-frequency heartbeat**
rather than re-running the same exhausted checks. It stays live so a returning operator can redirect it,
or so a pending obSCEne measurement (e.g. `sceUserServiceGetAgeLevel`, worklog 771) can be picked up when
it lands. The durable trail is in worklogs 758-783: one moved wall (rpf.cache), a fixed test race, an
explained regression, two honest-failure corrections, and six titles mapped to exact, tool-bound
blockers. Resuming productive work needs one of: the execution-tracer/reference-diff build, an obSCEne
result, or an operator's steer toward a specific title or tool.

## Gate state

No code changed - a characterisation that disproves the crt-init hypothesis for ASTRO BOT
(`START_MODULES=all` leaves the arena-init un-run and the run BACK), settles the retail frontier as
uniformly tool-bound, and drops the loop to a heartbeat pending a tool-build, a measurement, or steer.
`./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
