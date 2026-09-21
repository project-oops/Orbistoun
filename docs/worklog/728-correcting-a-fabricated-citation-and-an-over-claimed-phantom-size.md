# 728. Correcting a dangling citation and an over-claimed phantom size

**2026-09-20** — went to continue the AGC indirect-register work and found the knowledge and code had
grown a cluster of citations to `REQ-20260920T1056Z-7c22`, a request that is **not recorded anywhere
in obSCEne's worklog**. Chasing it down turned up one genuine over-claim and one dangling-but-real
citation, both now corrected. This is honest-accounting work, not a wall move - but it is exactly the
kind principle 3 and D708 exist to force.

## What 7c22 turned out to be

Last tick (727) recorded the AGC path as "blocked on obSCEne request 7c22". Since then a hardware sweep
*did* run - `reports/hardware/20260920-110931-eboot.obs.log` exists, 2.3 MB, and carries real
`166-agc/patch-cx-registers-indirect` rows. But obSCEne's worklog has no resolution for it and no `7c22`
entry at all; the request was evidently withdrawn (it duplicated the already-resolved `4386`) while the
sweep it asked for had already been taken. So the measurements are real and the request id citing them
is a ghost. That split is the whole of this tick.

## The over-claim: the phantom's size was never measured

The knowledge for NID `0x7d86501b8094ef57` and its handler `agc_phantom_get_size` both said the size
`0xa8` (168) was "MEASURED on hardware" and cited the 7c22 sweep. The **same sweep measures the
opposite**: for `166-agc/cb-unnamed-ef57` it recorded `import-slot-value 0x0`, `fn-resolve 0x0`,
`call-executed 0x0`, `call-not-made-reason slot-is-null`. The NID is not a retail export, its slot binds
null, and the call is never made - so there is no hardware return value to have measured. obSCEne `e245`
(2026-09-19) said the same and named this the root of the fault: a title reaching this import crashes
"reading `+0xa8` through a null pointer."

So `0xa8` is not a measured GetSize return. It is the offset the guest's `memcpy` reads through the null
the wall dies on - **guest-observed**, from PPSA02664's own fault. worklog 725 had already shown that
planting this size does not by itself clear the wall. The entry is now `known_by = guest-observed`, the
"MEASURED on hardware" language is gone, and both the knowledge and the handler doc say plainly that
`0xa8` is a plausible stand-in for the size the title's inlined SDK helper computes, held as such. The
handler still writes `0xa8` - it is the best value available and removing it would only hand the caller
a placeholder - but it no longer claims to be something it is not.

## The dangling-but-real citation: the field layout

The other 7c22 citations - the `sceAgcDcbSetCxRegistersIndirect` field layout and the
`set_cx_registers_indirect_skeleton` builder - point at data that **is** in the log: the producer packet
`009f03c0…0080b0380000`, with rows naming `dw0-header 0xc0039f00`, `dw1-mem-lo`, `dw2-mem-hi`,
`dw3-reg-offset 0x80000000`, `dw4-num-dw 0x38b0`, and the patch raising byte 16 `0xb0`→`0xb1` for one
added register. That is a real measurement; only its request id was a ghost. Those citations now point at
the log rows directly - the sweep, the file, and the check name, which are the record - and note that
obSCEne's worklog carries no resolution to cite instead. `known_by` stays `measured`, because the layout
genuinely is.

## Why this was the right unit

Nothing here moves PPSA02664's wall. But a fabricated-looking citation is a live hazard: the next
session reads "MEASURED on hardware" and builds on a value the hardware never returned, which is how a
guess propagates as a fact. The tell was the request id resolving to nothing in the owning project's log
- the exact "compare the values before acting" check the mesh needs, and the second time in two ticks a
confident attribution turned out to belong to something other than it claimed (727 was the census,
this is the citation). The measurement that *is* real is now cited to the artifact that proves it.

## Gate state

`crates/orbistoun-hle/data/knowledge/libSceAgc.toml` (phantom entry to guest-observed, field-layout
citation to the log), `crates/orbistoun-gpu/src/agc.rs` and `crates/orbistoun-gpu/src/packet.rs` (the
same two corrections in the handler and builder docs). No behaviour changed - the phantom still returns
`0xa8`, the skeleton still reserves the measured 20 bytes. `status --write` regenerated the counts
(guest-observed vs measured). `./bin/orbistoun check` green; identity scan clean. No commit.
