# D903 - A direct dispatch's modifier reaches its initiator

**Status:** decided
**Date:** 2026-10-19

`sceAgcCbDispatch(cb, x, y, z, modifier, ..)` writes the initiator `0x41` with the modifier's bits
`0xa038`, as an indirect dispatch's does (obSCEne `166-agc/dcb-dispatch-indirect`, three modifiers
measured). Among those bits is `CS_W32_EN` (bit 15): the program runs 32 lanes to a wave.

The measurement of the direct dispatch (REQ cd15) set none of these bits in any modifier it tried,
so the direct form is `assumed` to follow the indirect one. An obSCEne request confirms this.

**Why:** Grand Theft Auto V dispatches its wave32 compute programs with modifier `0x8001`. A fixed
`0x41` ran them as wave64, and every lane mask past lane 31 was wrong.

**Rejected:**
- Taking the wave width from the program alone: the hardware reads it from the initiator, and a
  program says nothing about the width it was compiled for.
