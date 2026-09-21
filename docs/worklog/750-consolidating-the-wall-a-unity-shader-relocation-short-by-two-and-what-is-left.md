# 750. Consolidating the wall — a Unity shader relocation short by two, and what is left to do

**2026-09-21** — a consolidation tick. After 740-749 narrowed PPSA02664's `CreateWorkload` fault to the
byte, this tick establishes the title's nature, refutes 749's asset-serving guess with the data already
in hand, and states plainly what the remaining unknown is and which tools it is behind. The point is to
leave the next session — or a better-equipped one — a clean handoff rather than another layer.

## The title, and why the loader is inlined

PPSA02664 is a **Unity / IL2CPP** title: the run opens `data.unity3d`, `globalgamemanagers.res*`, and
`Media/Modules/Il2CppUserAssemblies.prx`. The `"1234"` header the fault dies in is a PS5 AGC shader
object built by Unity's own PS5 graphics backend, which **inlines** the shader-object construction
rather than calling `sceAgcCreateShader` (the census confirms zero shader calls). So the relocation that
came up short is Unity's compiled code, not an orbistoun handler — which is why orbistoun's
`create_shader` list matching the observed three-of-five is a shared-lineage clue, not the site to edit.

## What 749's asset guess got wrong

749 proposed a truncated shader asset would give the loader a short relocation table. The descriptor
dump refutes it: **all five relative values are present** in the header — `+0x18=0xa8`, `+0x20=0x70`,
`+0x28=0x38`, `+0x30=0x60`, `+0x38=0x58` — so the header was copied whole from an asset served whole.
Three were relocated to `base+value`; two were left raw. The data arrived intact; the *relocation step*
added base to only the middle three. So it is not asset serving and not the copy — it is the loader's
relocation logic, applied to a header it received correctly.

## The state, stated once

- **The fault**: `memcpy(dst, src=0xa8, n=0x50)` — group 0's data pointer `[header+0x18]` holds the raw
  `0xa8` instead of `base+0xa8`; the register data it should point at is present at `base+0xa8` (748).
- **The shape**: five group pointers at `+0x18..+0x38`; relocated `+0x20/+0x28/+0x30`, raw `+0x18/+0x38`
  — the middle three done, the endpoints skipped. A `5` sits at `+0x4c`, the count a full loop would use.
- **Attribution**: Unity's inlined loader, not `sceAgcCreateShader` (not called), not the phantom (746),
  not the AGC patch family (742, measured no-ops).
- **What is unknown**: why the loader's relocation covered three of five here. On hardware the same
  Unity code relocates all five, so orbistoun feeds it a differing input — a bound, a count, or a value
  the loader derives one from.

## Why the two obvious tools cannot reach it

- **A construction watchpoint** needs the header's address before the relocating write, and the header
  is an ASLR'd heap object allocated at startup; `ORBISTOUN_HEAP_BASE` pins the base but not the offset
  (749), because the allocation count before it varies run to run and orbistoun has **no
  deterministic-execution control** to remove that variance.
- **A magic scan** for the loader found no `0x31323334` immediate in the graphics-code region (749),
  because the loader copies the header from asset data rather than testing a constant — so the code has
  no magic to grep for.

## What is left, ranked

1. **Measure the relocation spec.** The accurate-emulation answer to "which header fields are relocated"
   is a hardware measurement, not a guess. An obSCEne request that has `sceAgcCreateShader` relocate a
   header populated at `+0x18..+0x38` would establish whether all five are relocated on hardware, fix
   `create_shader`'s three-entry list (`agc.rs:253`, itself from a shader that never exercised the
   endpoints), and confirm what Unity's inlined loader must produce. This is the next concrete step and
   the one most in keeping with the standing "no hacks, measure" instruction; it needs the obSCEne
   request protocol learned first (not yet in hand).
2. **Deterministic execution.** A mode that fixes allocation order would make the construction
   watchpoint viable and unblock this whole class of early-write question — a real capability, but a
   larger build than one tick, and thread scheduling may resist it.
3. **Locate the loader by data, not magic.** The loader's `memcpy` of the header is traced; correlating
   the header's heap address (reachable via the copier's saved-`r14` slot) back to the `memcpy` that
   filled it would name the loader without the magic — fiddly, but tool-shaped rather than blocked.

## Gate state

No code changed — a run inspected for the title's file activity, an env-var review, and analysis.
`./bin/orbistoun check` is unchanged from 749 (green but for the same three generated-doc drifts from a
prior session's uncommitted `compat/PPSA02664-app0.toml` edit, not this tick). Identity scan clean. No
commit.
