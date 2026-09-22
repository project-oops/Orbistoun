# 788. PPSA28061's `sceAgcGetIsTrinityMode` is answered from the presented machine (base -> not Trinity), removing a placeholder; measured, it does not gate the libSceAmpr abort, which stays the hardware-faithful frontier

**2026-09-22** — worklog 787 moved PPSA28061 past the register-defaults descriptors to a guest-called
`abort` at 394 calls. This tick read that wall, tested the most tractable upstream hypothesis, and
records the honest result: the hypothesis is disproven, and the abort is the known
hardware-characterised mapper wall.

## The wall, read

The abort is a **system module** decision, not the eboot's. The engine at base `0x48...` (a shipped
module, on the evidence libSceAmpr - the async-compute runtime) is the only thing calling from `0x48`,
and it calls exactly two things: `sceKernelMapperGetParam(...) -> 0x80020006` (twice) then `libc::abort`.
Everything upstream - `sceAgcCreateShader -> 0x0`, `sceAgcDriverRegisterOwner -> 0x8a6c9018`, the
register-defaults, `sceAgcGetIsTrinityMode` - is the eboot at `0x40`.

obSCEne already settled the mapper (REQ-a2f9, b7e2): on retail `sceKernelMapperGetParam` is **not
resolvable for direct dynamic linking** and a cold call returns `0x80020006` filling nothing, so
orbistoun's `0x80020006` is **faithful**, and a2f9 framed the abort as "a game-engine branch". The
success path - four qwords the mapper fills at struct `+0x08/+0x10/+0x18/+0x20` - is still open and
holds no capture (libkernel.toml partial, REQ-...7a5d for a leg with libSceAgc mapped).

## The hypothesis, tested and disproven

`sceAgcGetIsTrinityMode` is a real AGC export (name from module-strings, not an inline NID) newly
reached this run and unimplemented, so it answered a non-zero placeholder. "Trinity" is obSCEne's own
axis name for the faster Prospero revision (beside `orbis`/`neo`/`prospero`), so the function is the
graphics twin of `sceKernelIsNeoMode`: a base-vs-Pro capability query. The hypothesis was that its
garbage placeholder read as "Trinity" and sent the eboot down a Pro-only compute path that invokes
libSceAmpr, where a base console never would.

Implemented it on the `sceKernelIsNeoMode` precedent - **arity 0, answered in the register from the
presented machine** (`platform() == Trinity`), so a base run gets `0` and a presented Pro gets `1`, no
hardcode. Measured: **verdict `same`, the abort is unchanged** at the same `sceKernelMapperGetParam`
site (standing 390 -> 391 of 394, one fewer stub). So the eboot invokes libSceAmpr regardless of the
Trinity flag - `sceAgcGetIsTrinityMode` is not the gate.

The implementation stays: it is the correct, per-machine answer for a real export, and a non-zero
placeholder telling a base console it is the faster revision is exactly the hazard `sceKernelIsNeoMode`
removes one layer down. It is `known_by = assumed` (the register-in / no-out-parameter shape is the
`IsNeoMode` precedent, confirmable by an obSCEne call on base and Pro hardware), not measured.

## Where the frontier actually is

Two upstream `assumed` answers remain the suspects for why the eboot reaches libSceAmpr's mapper path:

- **The register-defaults `count = 0`** (worklog 787) - if on retail those descriptors are populated,
  the eboot configures GPU context registers and may not take the compute path at all. Testing this
  needs a *populated* descriptor (a real default for register `0xe24f806d`/`0x6ac156ef` in set `0xd`,
  sourced from Mesa/ISA reset state), which the zeroed region cannot carry - the next buildable step.
- Or the wall is genuinely faithful and PPSA28061's next real motion needs the mapper's four-qword
  fill from a title-execution hardware trace (obSCEne-bound).

Either way `sceAgcGetIsTrinityMode` is closed and the sibling AGC titles (PPSA02664/03416, walling at
`image+0x3f8f0`) are the fresher, eboot-internal frontier to try the region mechanism against next.

## Gate state

Code changed: `orbistoun-gpu` implements `sceAgcGetIsTrinityMode` (arity 0, answered from the presented
machine), wired into the arity table, `implementations()`, and a `libSceAgc` knowledge entry
(`known_by = assumed`, `found_by = static`); a unit test pins the base answer to `0`. PPSA28061 verdict
`same` - the implementation is honest, not a wall-mover. `./bin/orbistoun check` green, worklog index
regenerated, identity scan clean. No commit.
