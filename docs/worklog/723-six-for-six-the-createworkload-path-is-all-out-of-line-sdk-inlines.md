# 723. Six for six: PPSA02664's CreateWorkload path is all out-of-line SDK inlines

**2026-09-20** — obSCEne resolved both requests filed for PPSA02664 (Alex Kidd in Miracle World)'s
graphics wall (`-af31`, `-c9e2`, sweep `20260920-082906`). Both `not-possible`, and together they
close a pattern that has been forming across worklogs 716-722.

## What the measurements settled

- **`-af31`:** neither `sceAmprAprCommandBufferConstructor` nor `sceAmprCommandBufferConstructor` is
  an export of `libSceAmpr` on the retail eboot leg. Dynamic resolution by name and by NID fails.
- **`-c9e2`:** neither `sceAgcDriverSetTFRing` nor `sceAgcDriverSetHsOffchipParam` is exported by
  `libSceAgcDriver`; export status identical to `sceAgcDriverAddEqEvent`.

That makes **six for six**. Every function PPSA02664's `CreateWorkload` path reaches that orbistoun
answers with a placeholder - `0x7d86501b8094ef57`, `sceAgcDriverAddEqEvent`, `sceAgcDriverSetTFRing`,
`sceAgcDriverSetHsOffchipParam`, `sceAmprAprCommandBufferConstructor`, `sceAmprCommandBufferConstructor`
- is confirmed **not a retail export** (obSCEne `-7c21`, `-3423`, `-c9e2`, `-af31`). Six independent
symbols, one code path, none in any export table, in a title that renders on hardware.

## What that means (and what it does not)

Six-for-six is not coincidence and, per D708, it is **not** evidence the title is broken. It is the
signature of **AGC SDK inline helpers compiled out-of-line**: a release build inlines these small
helpers, so it emits no import and there is nothing to resolve; this corpus build emitted them as
ordinary out-of-line imports. On the platform the title runs on they are inline code inside the
caller, which is why no export table anywhere lists them and why obSCEne - correctly - keeps
answering `not-possible` and recommending orbistoun **synthesise them in HLE**.

So the wall is orbistoun's (D708), and the path is synthesis. The standing difficulty is unchanged
and now proven exhaustively: synthesising them *accurately* needs their inline bodies, which are

- **unmeasurable** - obSCEne has now confirmed four separate times there is no export to call, and
- **un-copyable** - the provenance boundary rules out reading them from a reference emulator.

The one honest route left is **guest-oracle reconstruction** (worklog 719): the guest's own reads
tell orbistoun the minimal shape each helper must produce. It is the sanctioned oracle (principle 3),
and it is hard - the phantom's output lands in a global/heap location that arg-poisoning could not
find and that would need disassembling runtime-mapped guest code, which orbistoun has no tool to
dump. `sceAgcDriverAddEqEvent` is a special case within this: D705 already decided its completion is
not synthesised until GPU execution lands (roadmap phase 6), because posting a completion for work
that never ran is the plausible-output hack.

## Disposition

No measurement is coming that unblocks this - the requests that could have came back `not-possible`,
as designed. The wall is a synthesis problem gated on either a guest-oracle reconstruction (hard,
partially explored) or roadmap-phase-6 GPU execution (which is itself gated behind this same wall,
since no title submits until past `CreateWorkload`). This is the honest ceiling for PPSA02664's
reach, recorded so the next session starts from "synthesise the inline helpers" rather than
re-litigating whether the title is at fault.

## Gate state

Knowledge and ledger updated with the two resolutions; no code changed. `./bin/orbistoun check` was
green as of worklog 721. Identity scan clean.
