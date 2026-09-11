# 511. e4f1: fixed a transcribed opcode name obSCEne's disassembly caught; linkage HLE held

**2026-09-11** - monitoring loop; actioned obSCEne's REQ-...0230Z-e4f1 Part 1

obSCEne's `20260911-022248` sweep hit 100% on `166-agc` and disassembled retail `libSceAgc.sprx`,
then filed e4f1 asking orbistoun to ground the PM4 register opcodes and the shader-linkage HLE.

## Part 1 (packets.toml opcodes): done - and it caught a real error

Four of the five opcodes were already correct in `crates/orbistoun-gpu/data/packets.toml`:
`0x69 SET_CONTEXT_REG`, `0x76 SET_SH_REG`, `0x79 SET_UCONFIG_REG`, `0x2D DRAW_INDEX_AUTO`. The fifth
was **wrong**: `0x68` was named `SET_PREDICATION`. The standard AMD PM4 table (radv/Mesa `sid.h`) and
obSCEne's disassembly both have `SET_CONFIG_REG = 0x68`; `SET_PREDICATION` is `0x20`. So orbistoun had
`SET_PREDICATION` transcribed onto the wrong value, and `0x20` sat empty. Corrected `0x68 ->
SET_CONFIG_REG`.

Safe: nothing in code or tests dispatches on the name (grep clean; the file's own header says a wrong
name "mislabels a report and corrupts nothing"), and `cargo test -p orbistoun-gpu` stays green (4/4
vocabulary + doc tests). This is the "name it" loop motion on a decode table, external-grounded
(hardware + public AMD table), and outside the create-shader implementation hold.

## Part 2 (five linkage functions): held to the AGC-context pass, spec captured

`sceAgcCreateInterpolantMapping`, `sceAgcUpdateInterpolantMapping`, `sceAgcCreatePrimState`,
`sceAgcUpdatePrimState`, `sceAgcLinkShaders` are out-parameter-writing HLE - the same shape as
`sceAgcCreateShader` (worklog 508). Per the operator's "A" hold and b7e2 (the mapper is AGC-linked,
Earthion's chain is one context, worklog 509), these are implemented as **one coherent AGC-driver-
context pass** once the 3D sweeps settle, not piecemeal. e4f1 gives the behaviours that pass will
build to, worth keeping verbatim:

- `sceAgcCreateInterpolantMapping` (`$pdEV7bI6COI`): 32 QWORDs (256 B) for `SPI_PS_INPUT_CNTL_0..31`,
  default `((uint64_t)i << 32) | (0x191 + i)`; with VS+PS, maps VS exports (`vs+0x38`) to PS inputs
  (`ps+0x30`), `rc=0`.
- `sceAgcUpdateInterpolantMapping` (`$SbuY2jN+axQ`): in-place update of the active map, `rc=0`.
- `sceAgcCreatePrimState` (`$D9sr1xGUriE`): 32-byte prim-state block, topology in bits 0..4 of
  `sec_state + 0x14`.
- `sceAgcUpdatePrimState` (`$Y3ymLfZ1384`): updates topology (`sec_state+0x14`) and routing
  (`prim_state+0xc`), `rc=0`.
- `sceAgcLinkShaders` (`$MqAdbRMdNz4`): 256 B interpolants (`0x0..0xff`) + 32 B stage routing
  (`0x100..0x11f` = `0x000000020000029b`).

## Disposition

e4f1 acknowledged in the orbistoun inbox: Part 1 actioned (opcodes), Part 2 held to the coherent
AGC-context pass. No commit (operator commits); the one-line packets.toml fix is in-tree. Guard clean.

## Next

- Monitoring the C:\tmp inboxes continues.
- The AGC-context pass (create-shader object fill + queue + interpolant/prim/link) is now well-specced
  from 3c5e (object model) + e4f1 (linkage behaviours); it awaits the operator's go / the 3D work
  settling, then lands as one measured implementation.
