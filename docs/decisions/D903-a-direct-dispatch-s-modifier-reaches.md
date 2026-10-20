# D903 - A direct dispatch's modifier reaches its initiator

**Status:** decided
**Date:** 2026-10-19

`sceAgcCbDispatch(cb, x, y, z, modifier, ..)` writes the initiator `0x41` with the modifier's bits
`0xa038`, as an indirect dispatch's does (obSCEne `166-agc/dcb-dispatch-indirect`, three modifiers
measured). Among those bits is `CS_W32_EN` (bit 15): the program runs 32 lanes to a wave.

The measurement of the direct dispatch (REQ cd15) set none of these bits in any modifier it tried.
obSCEne gt21 measured the direct form across eight modifiers (`000-boot/gt21-agc-dispatch-modifier`,
report SHA-256 `03232c37…`). Every initiator was exactly `0x41 | (modifier & 0xa038)`, `0x8001`
giving `0x8041`, and the sixth argument did not reach it. The mapping is `measured`.

**Why:** Grand Theft Auto V dispatches its wave32 compute programs with modifier `0x8001`. A fixed
`0x41` ran them as wave64, and every lane mask past lane 31 was wrong.

**Rejected:**
- Taking the wave width from the program alone: the hardware reads it from the initiator, and a
  program says nothing about the width it was compiled for.
