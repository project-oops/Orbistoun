# D711 - a live submission's memory window is placed at the constant 64-bit base its vertex shader forms

**Status:** assumed
**Date:** 2026-09-24

## The question

A translated shader reaches guest memory through one storage buffer, the pipeline's `Window`: a
power-of-two span of words at a base, every access checked against it and anything outside refused
(worklog 561). Every caller that drew anything placed that window **by hand** - the capture tests
relocate the cube's vertex buffer to `0x0090_0000` and hand the pipeline a window spanning it
(worklog 571). The live submit path (`agc_driver::submit_described`) placed none, so its window was
the default - 64 words at address zero - and every vertex fetch a live guest's primitive shader made
was refused: both fully-owned baselines' draws collapsed and covered no pixel (worklog 823). Where
should a live submission's window go, and who says so?

The pipeline's own field documentation named the trap: deriving a window from the command stream
"needs a register vocabulary that says which register carries a buffer address, and nothing has
measured that", and inventing one "would produce a plausible frame drawn from the wrong memory"
(`REQ-20260914T2348Z-4e71`).

## The decision

**A pipeline built with `placing_window_from_shaders` places each submission's window at the constant
64-bit base its vertex-stage shader forms for its memory accesses.** The live submit path builds its
pipeline that way; every caller that places its window by hand keeps doing so.

- **The base is read out of the shader's own words, not a register.** The open-toolchain GL context
  does not pass its vertex buffer through user data: it patches the address into the shader as two
  literal moves (`s_mov_b32 s2, lo` / `s_mov_b32 s3, hi`) and forms each vertex's address as that
  pair plus the per-draw offset in `s8` plus the lane, carry-chained through `v_add_co_u32` and
  `v_add_co_ci_u32` (oops-sdk `tools/shader/vs-param3.s`). `constant_address_base` finds that pair:
  scalar registers the whole program writes exactly once, with `s_mov_b32` of a constant, and that no
  other destination - a scalar load's whole range included - touches; the base is the pair a
  `v_add_co_u32` takes as its low scalar source and a `v_add_co_ci_u32` takes one register up.
  A register the program computes, loads or reassigns is not a constant and names nothing.
- **The window is bounded by what the guest mapped**: the largest power-of-two span from the base, at
  most 2^16 words, that is wholly readable guest memory and does not cross four gigabytes. No base,
  or no readable span, leaves the window where it was.
- **The window gains its high half.** `Window.base` stays the low thirty-two bits, because a
  translated shader compares only the low half of an address (`Model::memory_base`); the high half is
  where the window's words are read from (`Window::address`), and `Window::spanning_address` refuses a
  window that would straddle a four-gigabyte boundary, where the low-half comparison would refuse
  words the window holds.

## Why this is not the invention `4e71` warned against

`4e71` refused a register mapping nobody had measured - a guess about which register means "buffer
address". This reads a constant the guest itself wrote into the program it runs, and places the window
exactly where that program will look. If the shader forms its addresses any other way, nothing is
found and nothing is placed; the failure is the old one (a window that reads nothing), not a frame
drawn from the wrong memory.

## What is assumed

- **That the first carry-chained add over a constant pair is the one whose memory matters most.** The
  cube's program has two - its vertex loads' pair (`0x2_0090_0000`) first, its canary store's
  (`0x2_0092_0000`) after - and the window serves one span, so the first is taken. A shader whose
  first constant-based access is not its vertex fetch would get a window over the wrong buffer; the
  refusals that followed would say so.
- **That a buffer does not straddle four gigabytes within the window**, the limit `memory_base` already
  documents; `spanning_address` refuses rather than risks it.
- **That the geometry shader's base serves the pixel shader too.** Every module is compiled against one
  window (worklog 635). A pixel shader's own constant-based accesses elsewhere - the cube's canary -
  are refused, which is the current behaviour, not a new loss.

## Consequence

The cube's first live frame shows a Gouraud-shaded face (worklog 824). Retires if the vocabulary ever
measures a register that carries buffer addresses (`4e71`), or when windows become per-draw.
