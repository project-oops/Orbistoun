# 505. Measured refusals do not move out-parameter walls — and Earthion gates on the mapper

**2026-09-10** - loop, closing the arc the operator's "can we move the wall without this data?" opened

500-504 walked from "everything is GPU-capture-blocked" to "the walls are a chain of out-parameter
fills, and the next one is a kernel struct." This tick tested the measured returns obSCEne produced
against the guests and settled what moves and what does not.

## The two measured refusals, tested

obSCEne measured both walls on hardware this afternoon:

- `sceKernelMapperGetParam` cold call → `0x80020006`, fills nothing (137-kernelcall/mapper-param).
- `sceAgcCreateShader` well-formed → `0x8a6c002f`, writes nothing - now **sentinel-verified**
  (0xc7 pre-fill, `out-before == out-after`) at **three** payload lengths (0xd8, 0x118, 0x108) on the
  `20260910-170907-eboot` leg. A null payload faults.

Returned each measured code to the guest:

| return | PPSA28061 |
|---|---|
| `sceKernelMapperGetParam` = `0x80020006` (measured) | abort at 334 (BACK) |
| `sceAgcCreateShader` = `0x8a6c002f` (measured) | abort at 334 (same) |
| `sceAgcCreateShader` = `0x8a6c003d` (the code the wrapper tests for) | abort at 334 (same) |
| `sceKernelMapperGetParam` = `0x0` (success prop) | 956 (FURTHER) |

**Only forcing success moves anything.** Every measured *refusal* leaves the guest exactly where the
unimplemented placeholder did. The reason is the same for both: these are out-parameter fills, the
measured hardware behaviour is refuse-and-write-nothing, and the guest needs the *fill*, not the
code (D556). So orbistoun can faithfully reproduce the refusal and it changes nothing, because a
shipping title does not get the refusal - it gets a successful fill from an initialised driver.

## Earthion gates on the mapper, not the shader

A specific finding worth pinning: in PPSA28061 the shader return is **irrelevant**. Both shader codes
above leave it at 334 because the call *immediately before* its abort is
`sceKernelMapperGetParam -> 0xf7ff0001`, which is what it actually tests. Earthion's wall is the
mapper call; the shader wall belongs to PPSA02664/03416. The corpus does not have one GPU wall, it
has a chain, and different titles die at different links.

## The root blocker under all of it: libSceAgc will not map

The `20260910-170907-eboot` leg - a real title launch, the best case for loading the GPU library -
reports `no GPU library is mapped | neither libSceAgc nor libSceGnm`. Every `dcb-*` builder crashed
(SIGBUS) for want of it; only `create-shader` resolved (through some other path) and it refused.
So even the eboot leg cannot exercise the AGC command builders, and the earlier PM4 encodings in the
knowledge (D565, `run-native-title.txt`) came from a run where the library *was* mapped, which this
one is not. This is exactly REQ-...1621Z-7a5d, now with a third leg's worth of evidence: the
successful fills every out-parameter wall needs live behind a libSceAgc that obSCEne cannot currently
get mapped.

## Where the arc lands

The honest answer to "can we move the wall without this data" resolved over 503-505:

1. **Mechanically, yes** - propping success moves the guest (334→956 on Earthion, confirmed live).
   It is a diagnostic, marked as one, and the guest then runs on a structure that is not real.
2. **Informatively, only where the fill is measurable** - and the fills route through libSceAgc,
   which is not mapped even on the eboot leg. Returning the measured *refusals* moves nothing.
3. **The one maybe-tractable step** is the mapper-param (a)/(b) fork (b7e2): if the mapper is a plain
   kernel object obSCEne can warm, its fill is measurable without the GPU library; if it is the AGC
   mapper, it joins the same block. Unresolved, and obSCEne's to determine.

So the vein the operator's question opened is real but narrow: one possible non-GPU fill (the mapper,
if it is kernel-warmable), and otherwise the whole out-parameter chain waits on a libSceAgc-mapped
leg. That is a sharper, truer statement of the block than 500's "blocked on GPU captures", and it was
worth the arc to reach it.

## Next

- b7e2 (a)/(b): the single determination that says whether the mapper is the exception or part of the
  rule. Monitor is armed for a changed mapper result.
- 7a5d: a libSceAgc-mapped leg remains the master key to every out-parameter fill in the chain.
- No orbistoun code moves a wall until one of those lands; returning measured refusals is faithful
  but inert, and forcing success is a diagnostic, not progress.
