# 509. b7e2 = case (b): the mapper is AGC-linked, and Earthion's chain is one context

**2026-09-11** - loop tick, reading b7e2's resolution and the operator's ongoing AGC sweeps

## b7e2 resolved: case (b)

obSCEne's answer to REQ-...1332Z-b7e2 (`resolved 2026-09-10T16:15Z`, sweep `20260910-170907`): on the
native title leg with AGC display active, `sceKernelMapperGetParam` **could not be resolved from
libkernel** at all; combined with the payload leg's cold `0x80020006`/ENXIO, this confirms **case
(b)** from worklog 504 - the mapper is **not an independent libkernel object**. "A title environment
without explicit AGC/driver registration cannot resolve or query it." It is the GPU/AGC mapper.

## What that means for Earthion

Earthion's abort chain is `sceAgcDriverRegisterDefaultOwner -> sceAgcCreateShader ->
sceKernelMapperGetParam -> abort`. b7e2 shows the last of those is **downstream of AGC driver
registration**, the first. So this is not three independent walls - it is **one AGC-driver-context
problem**: register the driver, create the shader, query the mapper, all against the same context.
The mapper's earlier promise as a "tractable non-GPU libkernel win" (worklogs 503-505) is withdrawn:
it was always part of the GPU arc.

## Why this sharpens the hold rather than breaking it

The operator's obSCEne work is now measuring exactly this context and nothing else:
`sceAgcDriverCreateQueue` (rc 0x0, `queue-header 38000000 03000000 00000200...`), `sceAgcInit`,
`create-shader`-with-queue (rc 0x0), `sceAgcDriverDestroyQueue` (0x8a6d0003), across sweeps landing
minutes apart. So the whole AGC driver context - the thing Earthion actually needs - is under active
measurement right now.

The create-shader object model itself is **stable** across `002219 -> 010855` (identical
guest-adjacent shape: object = arg1, +0x10 = arg2, +0x30 = +0x50 = 0, 0x2c changed in 0x130). So the
five-line create-shader handler of worklog 508 is still valid and would move Earthion **one step** -
from the create-shader wall to the mapper. But the *full* Earthion boot needs the driver context as a
coherent unit (register + queue + shader + mapper), which is what the operator is measuring. Building
create-shader alone moves one step; building the context is the win, and its shape is still landing.

## Revised implementation scope (held)

Worklog 508's create-shader handler stays the first increment (moves past create-shader on measured
data). Behind it, the eventual unit is the **AGC driver context**: `sceAgcDriverRegisterDefaultOwner`,
`sceAgcDriverCreateQueue` (queue object, `38 00 00 00 03 00 00 00 ...`), `sceAgcInit`, and the mapper
query - implemented together so the chain is coherent rather than piecemeal. Held until the operator's
context measurements settle, per the operator's "A".

## Next

- Hold, lean. The productive work is on obSCEne's side (measuring the AGC context); orbistoun's move
  is to implement the whole context once, when it settles - create-shader first, then the driver/queue/
  mapper around it.
- No `learn` this tick: knowledge-file writes were denied during the hold, so measured facts
  (create-shader stable, driver-create-queue rc 0x0 + header, mapper = case b) are recorded here.
