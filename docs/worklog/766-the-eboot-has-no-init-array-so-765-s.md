# 766. The eboot has no init-array, so 765's axis was wrong; the redirect is runtime input-init that runs after the poll, and the trigger is the real question

**2026-09-21** — worklog 765 named "init-array execution" as the axis of PPSA04263 (Grand Theft Auto V)'s
null-vtable and `sceKernelGetProcParam` as the next gap, on the strength of `ORBISTOUN_START_MODULES=all`
moving the wall. Parsing the eboot's own dynamic section refutes that framing. This corrects it before it
sends the next session down a dead end, and repoints at what the fault actually waits on.

## The eboot has no init-array

The inner ELF's `PT_DYNAMIC` (241 entries, vendor and standard tags side by side) carries, for
initialisation:

| tag | value |
|---|---|
| `DT_INIT_ARRAY` (`0x19`) | `0x0` |
| `DT_INIT_ARRAYSZ` (`0x1b`) | `0x0` |
| `DT_PREINIT_ARRAY` (`0x20`) | `0x0` |
| `DT_INIT` (`0xc`) | `0x10` (not a code address; effectively unused) |
| `DT_RELA` / `DT_RELASZ` | `0x5e7fcd0` / `0x3f4710` (≈172790 entries - the relocation count) |

So there is **no eboot init-array to run**, and nothing for a loader-side init pass to execute on the
main image. The relocation machinery is ordinary `DT_RELA`, already applied in full (worklog 761).

## Yet the default is constructed - so ctors already run

The fault-time dump (765) has the default handler at `image+0x5b37f30` holding a real vtable
(`image+0x3c729d0`). With no init-array, that construction runs through the **guest crt / a
function-local (lazy) static** on first use - a path orbistoun already reaches by entering at
`image.entry()`. Construction is not the missing step.

## What the redirect actually is

`image+0x1965fbc` (`field_0x2d0 = real_handler[0] ? : default`) sits inside a function that opens pad
devices (`0x3391020`), initialises four pad objects at `image+0x5aaee50` (stride `0x100`), poisons a
failed one with `0xff`, and only then writes the redirect. Its neighbours in `image+0x1965a40 ..
0x1966040` all operate on the same `element[index].field_0x2d0` handler through guarded virtual calls
(offsets `0x50`, `0x58`, `0xd8`). This is **runtime input-subsystem bring-up**, not a static constructor
- it runs when the input stack is initialised, not at process start.

The fault is the input **poll** (`image+0x1967680`, called from the update routine at `image+0x19669ad`)
reaching `element[0].field_0x2d0` while it still holds its **static-initialiser relocation**
(`image+0x5b37e98`, an un-constructed handler) because the **bring-up redirect that would point it at the
constructed default has not run yet**. On hardware the bring-up precedes the poll; here the poll wins.

## Correcting 765

`START_MODULES=all` runs every *placed module's* `DT_INIT_ARRAY`. The eboot has none, so that knob never
touched the eboot's path - it ran a **module's** init, which dereferenced `sceKernelGetProcParam`'s stub
and faulted there first. That is a real gap on the module-init path but a **red herring for this fault**:
implementing `GetProcParam` would not make the input bring-up precede the poll. 765's "init-array is the
axis" and "GetProcParam is the next gap" are both withdrawn for PPSA04263.

## The real question, and how to answer it

What triggers the input bring-up (the `0x1965fbc` function) on the console, and why does orbistoun's run
reach the poll first? The tractable next step is a **trace pass**, not more disassembly: enumerate the
input-init imports GTA calls in a normal run (`scePadInit`/`Open`, `sceUserServiceInitialize` and
relatives), and find which one, stubbed or no-op here, is the bring-up's trigger - or whether an
"is-initialised" flag the update routine at `image+0x19669ad` reads is answered wrongly, letting the poll
run before bring-up at all. The update routine gates its `call 0x1967680` behind a stack of boolean
globals; one of them is the guard that should be false until bring-up completes.

## Gate state

No code changed - a characterisation that parses the eboot's dynamic tags, withdraws 765's init-array
framing and its `GetProcParam` next-step for this title, and repoints at the input bring-up trigger as a
trace question. `./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
