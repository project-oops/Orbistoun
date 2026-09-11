# 510. Actioned 4b1a: obSCEne's compute-dispatch stream decodes whole; execution is the gap

**2026-09-11** - loop (now monitoring the C:\tmp request mesh); actioned obSCEne's REQ-...2205Z-4b1a

obSCEne asked orbistoun to decode (and execute) the hardware-verified RDNA2 compute dispatch it ran
end-to-end on live Oberon silicon (sweep `20260910-214758`, `alu-val 0x23456789`, `fence beefcafe`).
The DCB and shader are constructed in obSCEne `src/probe/sections/agc.c` (commit `ed64a83`,
`check_agc_compute_dispatch`), so the byte streams came from there, not a log dump.

## Decode: fully supported, on real bytes

**Shader** (the 15-dword / 10-instruction GFX10 compute kernel from `agc.c`: `s_waitcnt`,
`s_mov_b32` x2, `v_mov_b32` x3, `v_add_nc_u32`, `global_store_dword`, `s_waitcnt`, `s_endpgm`).
Written to `compute.gcn` and run through the validated decoder:

```
./bin/orbistoun cli shaders <dir>
  shaders      1 of 1 complete
  instructions 10 of 10 translatable
  no blockers - every instruction seen is supported
```

Every instruction the real hardware kernel uses decodes and translates - including the two the
`measured_shader` anchor did not cover (`v_add_nc_u32` VOP2, `global_store_dword` FLAT/GLOBAL).

**PM4 DCB** (SET_SH_REG x13, DISPATCH_DIRECT, RELEASE_MEM, then NOP padding). Every opcode is named
in `crates/orbistoun-gpu/data/packets.toml` and walks by its count field:

| dword | opcode | name (packets.toml) | count | walks |
|---|---|---|---|---|
| `0xc0017600` | 0x76 | SET_SH_REG | 1 | yes (3 dwords) |
| `0xc0031500` | 0x15 | DISPATCH_DIRECT | 3 | yes (5 dwords) |
| `0xc0064900` | 0x49 | RELEASE_MEM | 6 | yes (8 dwords) - matches the D565 builder capture |
| `0xffff1000` | 0x10 | NOP | 0x3fff | the known non-closer (measured_packets), used here as trailing prefetch padding |

So the whole submitted command stream is decodable and named; only the trailing NOP padding is the
already-documented count-is-not-a-length case, and it is padding, not a command.

## Execute: the gap, named precisely

4b1a's parts 2 and 3 - run the ALU (`0x12345678 + 0x11111111 = 0x23456789`), commit `global_store`
to simulated Onion memory, signal the EOP fence (`0xbeefcafe`) - are **not built**. That is the
translate/execute layer (`orbistoun-translate`, GPU execution), which `docs/ROADMAP.md` puts at
Phase 6 and which has not begun. `cli shaders` reports the instructions *translatable*; actually
executing them and committing a writeback is the unimplemented step. This is the honest
decode-vs-implementation diff 4b1a's acceptance asked for: **decode = done, execute = Phase 6.**

## Why this was safe to action under the hold

The hold is on writing GPU *implementation* (the create-shader object fill, the AGC driver context).
This was a **decode-and-report** against obSCEne's external-oracle bytes - the same safe kind as
worklog 506, no emulator behaviour added. It answers a sibling's request with what the validated
decoders find, and confirms the decoders cover a real hardware compute kernel end to end.

## Next

- 4b1a resolved in the orbistoun inbox with this result.
- Execution (Phase 6) remains the frontier; when it begins, this exact stream is the first
  hardware-verified end-to-end benchmark (obSCEne measured `alu-val 0x23456789`, `fence-hit 1`), so it
  belongs as the acceptance test for the execute layer.
- The prose CI gate (d529) is red at 176 sites / 32 unlisted files from the HLE commit - raised
  separately as needing a dedicated committed pass, not a monitoring-tick fix.
