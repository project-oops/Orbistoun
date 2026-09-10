# 501. The naming avenue is closed, not untried - and obSCEne's AGC names are already ours

**2026-09-10** - loop, continuing 500

500 said the frontier is blocked on obSCEne's `8ef4` full captures or the loader deny. This tick
tested the one move that needed neither - naming the bare hashes the loop asks me to name - and
established that it is exhausted, with a receipt for each source tried.

## What the loop's "name a bare hash" move actually has left

The call-weighted `worklist` reports **4 of 341** distinct imports called across all 10 runs still
have no name. Statically the deepest reacher, GTA V (`PPSA04263`), imports **1410** symbols, **1002
unresolved**, and **629** of those unnamed - almost entirely `libSceAgc`, the GPU command library.
So the naming question is really: can the 629 unnamed `libSceAgc` functions be named from anything
reachable here?

## Four sources, each tried, each a receipt

| source | result |
|---|---|
| `cli names` brute force | **3,911,843,959** generated candidates across 11 patterns + 24,192 module strings + 3,018 published names, **0 of 629 named** in 267s. The vocabulary already carries the graphics domain (Dcb, Draw, Dispatch, Dma, Marker, Viewport, Scissor, Shader, Blend, DepthStencil, Sampler, RenderTarget, Fence, Query, Rewind, Flip, Stall, Acquire, Release, Jump ...). |
| obSCEne `src/probe/sections/agc.c` | names **18** AGC functions it calls. Computed each NID and matched against GTA V's imports: **all 18 are already named in orbistoun's DB** (`sceAgcCbNop`, `sceAgcCreatePrimState`, `sceAgcDcbDmaData`, `sceAgcQueueEndOfPipeActionPatchAddress`, the `Patch*` family ...). Zero new. |
| SELFish `crates/selfish-nid/tests/known-pairs.txt` | one AGC entry, `sceAgcCreateShader` - already ours (it is the named wall). |
| the module's own strings | imported by NID, not string; the eboot carries none of these names. `names` confirmed 0 from module bytes. |

**orbistoun's AGC symbol coverage already subsumes obSCEne's AGC usage.** That is worth stating
plainly: obSCEne cannot name a `libSceAgc` import that orbistoun cannot, because it only knows the
ones it calls, and we already have all of those. Cross-referencing the sibling is not a lead here.

## So the 629 are named by nothing that exists in OOPS

They are functions a retail engine imports that no probe calls and no English-word grammar spells.
Naming them further needs a *source* - not more brute force, and not a sibling. The 3.9-billion
sweep is not "not tried hard enough"; it is the combinatorial ceiling of a vocabulary that already
has the right domain words. A `not-in-what-was-tried` miss at that scale is as close to closed as
this method gets.

## And it does not move a wall anyway

Only 4 called-but-unnamed across every run, none in the `worklist` top 25 (all >4,563 calls are
named), none on a wall - every wall is a *named* import (`sceAgcCreateShader`,
`sceCommonDialogInitialize`) or a deep fault inside the title. So the unnamed 629 are static
imports the guest never reaches before it dies at the GPU wall. Naming them would buy legibility
for a future capture, not a wall.

## What this sharpens about `8ef4`

When a full command buffer lands, the calls inside it that orbistoun cannot name are exactly this
set. A capture that also carried the resolved symbol for each identified call would be worth more
than bytes alone - but obSCEne resolves by the same link-map orbistoun would, and we already hold
its AGC names, so the realistic answer is that a *whole buffer's* opcodes decode structurally
(worklog 499 proved the header does) regardless of the leaf function names. The names are a
red herring for moving the GPU wall; the *bytes* are the thing. That is the receipt that keeps me
from re-filing a naming request against obSCEne: it could not answer it.

## Next

- Unchanged from 500: `8ef4` full captures (the bytes, not the names), or the loader deny lifting.
- Naming is retired as a move until a title actually *calls* an unnamed AGC function at runtime -
  at which point it is on a wall and worth a targeted, single-name effort, not a sweep.
