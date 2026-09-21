# 731. Mapping the CreateWorkload path: AGC-driver calls ruled out, sceAgcInit is load-bearing

**2026-09-20** — with the workload object shown to be a zeroed allocation (worklog 730), the question is
what should fill it. This tick mapped the calls CreateWorkload actually makes, ruled out the ones the run
report flags as gaps, and found the one whose answer the build genuinely turns on - not the one that
looked like the gap.

## The call sequence, read off the trace

The improved register dump made the recent-call tail legible. CreateWorkload runs, in order:
`sceAgcDcbEventWrite` ×2, `sceAgcCbNop`, `sceAgcDcbDmaData`, `sceAgcCbReleaseMem`,
`sceAgcDcbWaitRegMem`, the phantom GetSize (`0x7d86501b8094ef57`, writes 0xa8 to its out-param),
`sceAgcDmaDataPatchSetDstAddressOrOffset`, then the `0x42d90` loop of `memcpy` /
`sceAgcSetCxRegIndirectPatchAddRegisters` that faults. Alongside them the report flags four unresolved
calls on the path: `sceAgcDriverAddEqEvent` (×2), `sceAgcDriverSetTFRing`, `sceAgcDriverSetHsOffchipParam`,
and one unnamed NID, `0x53bbd82b51d172db`.

## The flagged AGC-driver calls are not the wall

The report ranks the three unimplemented `libSceAgcDriver` calls first and says to implement them. The
report's own methodology says test that before writing code, so
`ORBISTOUN_RETURN=…AddEqEvent:0x0,…SetTFRing:0x0,…SetHsOffchipParam:0x0` answered all three with success.
The fault did not move - still `read of 0xa8`. So they are ruled out as the cause, the same way worklog
562's four candidates were, and three functions did not get a synthesised return written for them on a
guess that would not have helped. obSCEne already settled that all three are non-exports (c9e2, 3423), so
there was nothing to measure and no reason to invent one.

## The unnamed NID is sceAgcInit, and its answer is load-bearing

`0x53bbd82b51d172db` is not new - obSCEne resolved it as `sceAgcInit` (its `platform.h`), and orbistoun
already carries the entry. It is one of this library's **alias identifiers**: the name does not hash to
this NID (the hash mechanism cannot reach it), five published tables carry it against `sceAgcInit`, and
the knowledge records it `known_by = published` for exactly that reason. So it stays unnamed in the
trace by design, not by omission - `vendor.toml` is a hash-search vocabulary and cannot hold an alias a
hash will not reproduce.

What is new: its **answer is load-bearing**. Under `ORBISTOUN_RETURN=…:0x0` the guest regresses - verdict
BACK, faulting earlier at `read of 0xf7ff0039`, a placeholder read at image+0x3ac99, instead of reaching
the 0xa8 wall. My first read of that was "the return is a handle, not a status, and it is a non-export so
there is nothing to measure" - and that was wrong, caught by checking obSCEne before believing it. `b7e4`
(resolved 2026-09-15) **already measured** `sceAgcInit`: it is `sceAgcInit(state, v)`, arity 2, not 0; it
returns `0x0` for version 13 and `0x8a6c0004` for every other version; and it gates the library
(`sceAgcCreateShader` before it SIGSEGVs). So the return is measured and `0x0` is *correct* for v=13. The
reason forcing `0x0` regresses is the other argument: `sceAgcInit` fills a **state buffer at arg0**, and
answering success while that buffer stays zero tells the guest the library is initialised when its context
is not - worse than the placeholder, which the guest reads as failure and routes around. `b7e4` captured
the return and the gate but **not** the bytes `sceAgcInit` writes into that state, and those bytes are the
AGC context every later builder - and the zeroed workload object - is read out of.

## Where this leaves the wall

The zeroed workload is downstream of a `sceAgcInit` whose state buffer orbistoun never fills. That reframes
the next step from "populate the object directly" to "fill the AGC context `sceAgcInit` produces, then the
workload build reads a real object out of it". The return and the version gate are already measured
(`b7e4`); the one missing measurement is what `sceAgcInit(state, 13)` writes into that state buffer, and it
is measurable the same way `3c5e` measured `sceAgcCreateShader` one call later - so this tick filed obSCEne
request `a3f0` for exactly that dump. Implementing the return alone before it lands would move the wall
backwards, so the honest state is: return known, gate known, state bytes pending, no code until they
arrive. The knowledge now records all of that, and the load-bearing call is separated from the three that
merely looked like gaps.

## Gate state

One data change: `crates/orbistoun-hle/data/knowledge/libSceAgc.toml`, the `sceAgcInit` entry gains the
return-sensitivity finding and the AGC-driver ruling-out. No code, no behaviour change. `./bin/orbistoun
check` green; identity scan clean. No commit.
