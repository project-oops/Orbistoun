# D781 - sceAgcCreatePrimState writes its register lists whole

**Status:** decided
**Date:** 2026-10-09
**known_by:** assumed (`166-agc/create-prim-state` measured the fields named below and zero
around them, on buffers the probe zeroed, which cannot tell a zero written from one never touched)

`sceAgcCreatePrimState(prim_state, sec_state, ...)` writes the whole of both register lists it
fills: the first 0x10 bytes of `prim_state`, two `(offset, value)` pairs, and the first 0x100 bytes
of `sec_state`, thirty-two. Every byte the measurement did not name is written zero, so a list the
title copies holds the measured pairs and `(0, 0)` pairs and nothing the caller left there.
`sceAgcUpdatePrimState` still changes only the topology field.

**Why:** PPSA02664 copies both lists from uninitialised stack buffers straight into a
`SET_CONTEXT_REG_INDIRECT` table, `prim_state` as two pairs and `sec_state` as thirty-two, adjacent
on its stack 0x100 apart. Left as the caller prepared them, the pairs carried stale stack words,
which wrote arbitrary context registers; one zeroed `CB_COLOR0_BASE_EXT` and moved a colour target
into unmapped memory on some runs and not others. A probe that fills the buffers with a pattern
before the call retires the assumption.
