# 503. The wall moves mechanically — and the next one is a kernel struct, not GPU-blocked

**2026-09-10** - loop, prompted by the operator asking "are we not able to move the wall without this data?"

A fair challenge to the "everything is blocked on obSCEne's GPU capture" framing of 500-502. Tested
it against the furthest title on the current tree, and the honest answer is more useful than either
"yes blocked" or "no not blocked".

## Two senses of "move the wall", and they have different answers

- **Move the guest to the next wall** (the project's `FURTHER` metric): *yes*, without any GPU data.
  A prop does it, and it is a sanctioned technique - the 1-bit oracle (CLAUDE.md) and the
  `[experiment]` slot (D227/D312/D548/D555). Confirmed live this tick.
- **Emulate the GPU** (translate command streams, render): *no*, that needs the captures. Nothing
  here changes that.

The trap is conflating them, which 500-502 half-did. Separated, the picture opens up.

## The wall is a chain of unwritten out-parameters, not one GPU call

PPSA28061 (Earthion) honest run, current tree: stops at `abort` after **334 calls**. The call
*immediately before* abort is not the shader - it is
`libkernel::sceKernelMapperGetParam(...) -> 0xf7ff0001`. The abort tests that return (D643):

```
ORBISTOUN_RETURN=sceKernelMapperGetParam:0x0   →   334 calls → 956 calls,  26 → 59 imports,  verdict FURTHER
```

Confirmed live, not just cited. The runtime flagged it correctly: *"this run altered the program, so
getting further may mean"* the guest is working on something fake - the D227 marker doing its job.
Past the prop the guest reaches `sceAgcCreateShader` again with **real material** (arg1 header
`31 32 33 34 18 00 00 00 d8 ..`, arg2 RDNA bytecode `02 00 a0 bf ..`), runs 622 more calls, and
aborts inside `printf`/`memmove` - printing a diagnostic and giving up, because the mapper struct it
read back was zeros.

So the walls are a **chain**: `sceAgcCreateShader` (out-param shader object) →
`sceKernelMapperGetParam` (out-param mapper struct) → deeper. Each is an unwritten out-parameter, and
propping any of them moves the guest past it to the next - mechanically. Informatively, each prop is
empty on its own (D227/D556): the guest then reads a structure that is not one.

## The distinction that matters: which out-params are measurable *now*

Not all of them are GPU-capture-blocked. That is the correction.

| out-param | library | obSCEne can dump it? |
|---|---|---|
| `sceAgcCreateShader` shader object (fields +0x30, +0x50) | libSceAgc | **No** - libSceAgc is not mapped in obSCEne's process (472 symbols absent, 4 captures); needs a leg where it loads, which the `swp20260910-141829` sweep just showed is hardening/GPU-lib blocked |
| `sceKernelMapperGetParam` 56-byte struct | **libkernel** | **Yes, today** - libkernel is always mapped; `kernelcall.c` already `sceKernelDlsym`-calls libkernel functions and reports bytes |

So the very next wall past the shader is **not** blocked on the thing the shader is blocked on. It is
a plain libkernel struct dump obSCEne can do in its own process. Filed as
`REQ-20260910T1332Z-b7e2`: call `sceKernelMapperGetParam` with a size-prefixed 56-byte buffer
(first quadword `0x38`) and emit the filled bytes. When they land, orbistoun fills the struct for
real and Earthion advances ~3x on *measured* data, no GPU capture involved - the first genuinely
informative wall-move in a while.

## Why this was worth a tick even though no wall moved *for real* yet

Because "blocked on GPU captures" was too broad and would have parked work that is actually
reachable. The honest shape is: the GPU *rendering* is capture-blocked, but the *startup chain* the
titles die in is a sequence of out-parameter structs, and at least the next one is an ordinary kernel
struct obSCEne can measure without any of the GPU machinery. The corpus frontier has a tractable
libkernel-shaped piece in it, and it was one honest run away.

## Next

- b7e2's 56 bytes → implement `sceKernelMapperGetParam` to fill the struct → re-run Earthion and see
  the *real* next wall (not the propped-fake one), which is the first informative move past 334.
- If that next wall is another libkernel out-param, the same pattern repeats and is tractable; if it
  is the shader object, we are back to the GPU capture - but we will have walked the guest there on
  measured data instead of guessing.
- The shader wall itself stays what 502 said: obSCEne's `166-agc` builder-call pivot, pending a leg
  where libSceAgc loads.
