# 514. 9a41: the shader-linkage functions, Type 0 queue accept, and draw-stream decode

**2026-09-12** - operator said "everything in 9a41", commit authorized

obSCEne's `9a41` handed over the graphics pipeline it verified end to end on hardware (sweep
`20260912-003916`: NGG primitive shader, rasterisation, pixel shader, MRT0 wrote `0xff0000ff`). The
acceptance is *decode without warnings + linkage implemented/ground-tested* - not full rasterisation,
which is Phase-6 rendering and stays unbuilt. Delivered to that bar, all measured, all tested.

## The five shader-linkage functions (`libSceAgc`, e4f1 Part 2)

Implemented in `agc.rs` from the measured behaviours in `166-agc/link-shaders` and
`166-agc/update-prim-state`:

- `sceAgcCreateInterpolantMapping(mapping, vs, ps)` - fills 32 quadwords, default
  `(i << 32) | (0x191 + i)`. Measured: the table came back `0x191`, `0x1_0000_0192`, ...
- `sceAgcUpdateInterpolantMapping` - same table, in place.
- `sceAgcCreatePrimState(prim, sec, null, vs, topology)` - topology into the low five bits of
  `sec_state + 0x14` (read-modify-write, preserving the rest).
- `sceAgcUpdatePrimState(prim, sec, topology)` - same field; measured `topo-orig 4 -> topo-updated 1`.
- `sceAgcLinkShaders(link, sec, null, vs, ps, ...)` - the 256-byte interpolant table at `+0x0` plus
  the routing quadword `0x0000_0002_0000_029b` at `+0x108`. Both halves measured.

Where obSCEne's behaviour was only partly specified - the vs/ps-specific interpolant remap, the
`prim_state + 0xc` routing word - the code writes the measured default and leaves the rest as the
guest prepared it, rather than inventing a layout (principle 3). Five unit tests pin the table, the
routing quadword, and the topology read-modify-write.

## Type 0 queue accept (`libSceAgcDriver`)

`sceAgcDriverCreateQueue(type, out, flags)` now declared and accepts type 0 (and 3), returning the
measured `rc-create 0x0`. It deliberately does **not** fabricate the queue object into `*out`:
obSCEne measured the object's header but not where the call places it (no `3c5e`-style pointer
distance), so writing a pointer there would be plausible-output. Return honest, out-parameter waits
on that measurement. No guest reaches this call yet (the corpus stalls earlier at
`sceAgcDriverRegisterOwner`), so accept-and-return is exercised by obSCEne's rc check and tests.

## Type 0 draw stream decodes without warnings

`tests/graphics_draw.rs` assembles a Type 0 draw from 9a41's exact opcode/register list -
`SET_CONTEXT_REG` (CB colour target, SPI PS input), `SET_UCONFIG_REG` (VGT primitive type, GE_CNTL),
`NUM_INSTANCES`, `DRAW_INDEX_AUTO` - and asserts the walk consumes it exactly with no unknown packet,
and that the five register writes are captured with the request's values. That is the decode-side
acceptance: an unknown opcode or a bad length is what a "warning" would be, and the walk reports
neither. (`packets.toml` already named every draw opcode; the `0x68 SET_CONFIG_REG` fix from e4f1
was the last gap.)

## What is deliberately not here

Full draw **execution** - NGG primitive-shader waves, the scan converter, pixel-shader launch, the
MRT0 export obSCEne saw write red - is the Phase-6 rendering engine and is not built. 9a41's "why"
describes the hardware result; its acceptance for orbistoun is decode + linkage, which is what this
is. Claiming a rendered triangle would be the exact plausible-output the project forbids.

## Tests and state

`cargo test -p orbistoun-gpu`: 31 lib + 2 graphics_draw + the existing suites, all green. PPSA28061
honest run unchanged at 396 calls (create-shader still `0x0`), no regression; it still stalls at
`sceAgcDriverRegisterOwner`, which needs `7b3c`. So 9a41's layer is implemented and tested but sits
behind Earthion's current wall - it moves the corpus when `7b3c` lands and the guest reaches it.

## Next

- Commit as legboots (guard --cached first). Push stays blocked on the prose gate (d529), the other
  session's to clear.
- Resolve 9a41 in the inbox with this.
- `7b3c` (RegisterOwner/mapper) remains Earthion's actual unblock.
