# D781 - sceAgcCreatePrimState writes its register lists whole

**Status:** decided
**Date:** 2026-10-09
**known_by:** measured (`166-agc/create-prim-state`, arms `topo-4-a5` and `topo-17-a5`,
`reports/hardware/20261009-104652-eboot.obs.log`: buffers filled with `0xa5` before the call)

`sceAgcCreatePrimState(prim_state, sec_state, ...)` writes the whole of both register lists it
fills, and nothing past them: the first 0x10 bytes of `prim_state`, the pairs `(0, 0)` and
`(VGT_GS_OUT_PRIM_TYPE, out)`, and the first 0x18 bytes of `sec_state`, the pairs `(0, 0)`,
`(GE_USER_VGPR_EN, 0)` and `(VGT_PRIMITIVE_TYPE, topology)`. The zero words are written, not
left: over the `0xa5` fill they read back zero, and the fill resumes at 0x10 and 0x18. The topology
dword is written whole.

**Why:** PPSA02664 hands both buffers over uninitialised from its stack and copies 0x18 bytes of
`sec_state` into a register table straight after the call. Left as the caller prepared them, the
pairs it copies carried stale stack words into context registers. The 0x100 bytes it later copies
out of the same buffer are the interpolant table `0x71040c4df8235e1d` writes there, not this list.
This was first decided as an assumption that both lists ran to 0x100 bytes; REQ-20261009T0500Z-ps01
measured the extent and replaced it.
