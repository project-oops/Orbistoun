# D419 - Five more libkernel vaddrs behaviourally confirmed from the second payload run


**measured** - 2026-08-31

obSCEne's D274 export candidates came back from hardware. The second payload run's `139-exports`
section confirmed **twelve** vaddrs where the first confirmed eight, and the five new ones each match
what orbistoun's table already listed as a candidate - so this is confirmation, not correction:

| symbol | vaddr | note |
|---|---|---|
| `sceKernelGetTscFrequency` | `0x1cf30` | was *refuted* last run; obSCEne's exact-match check was too strict for an 79 Hz boot-calibration drift, so D274 widened it to a band and it now confirms |
| `getsid` | `0x1590` | new candidate |
| `sceKernelReadTsc` | `0x1cfa0` | new candidate |
| `sceKernelGetProcessTimeCounter` | `0x1d010` | new candidate |
| `sceKernelGetProcessTimeCounterFrequency` | `0x1d030` | new candidate |

All five are promoted from candidate to `confirmed` in `libkernel-vaddrs.txt` (7→12 confirmed). The
CLI's confirmed count is computed, not hardcoded, so nothing else needed touching; the firmware
crate's `provenance_marks_the_confirmed_exports` test still passes.

Worth recording about the run itself: it is a **payload** build, so 363 of 543 results are `fail`
under `900-surface/*` ("none of this library is present"). That is not a regression - the elfldr
payload path resolves only the base+vaddr exports, never a full import table, so the whole-surface
census legitimately finds no libraries. Those same probes resolve in an eboot/module build. The one
number that matters here is `139-exports`: 12/12 pass, `0xc` confirmed. No byte dumps this run
(payload build), so no new struct layouts to absorb - the SwVersion layout from the prior run
(D416/refinement) remains the last structural datum taken.

