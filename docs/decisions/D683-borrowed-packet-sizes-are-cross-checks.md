# D683 - Borrowed packet sizes are cross-checks, never sources

**Status:** decided
**Date:** 2026-09-14

## The choice

Another project's table of AGC command-packet sizes is read **after** we measure, never
instead of measuring. Every size in `crates/orbistoun-hle/data/knowledge/libSceAgc.toml`
carries `known_by = "measured"` and cites the obSCEne check that produced it. Where the
borrowed table agrees, that is recorded as corroboration. Where it disagrees, **both figures
are recorded and the row stays open** - it is not reconciled by preferring either side.

## Why it matters

prosper, an independent high-level emulator of the same target, publishes a packet-size audit
with a per-row confidence column. Copying it would have saved this measurement entirely, and
it would have been the wrong trade twice over.

**First, because their numbers are derived differently and the difference is load-bearing.**
Their figures come from what guests *reserve* - inferred by watching a title run out of
command-buffer space - not from what the library answers. That method is excellent at finding
where their own emitter is too large, and it is structurally unable to see a builder no title
in their corpus exercises. Our figures come from calling `sceAgc*GetSize` on the real library
and then calling the builder through a writer we own. Reservation, cursor advance and write
extent are reported separately and all three agree on every builder measured. These are two
unrelated routes to the same class of fact, which is exactly what makes agreement worth
something.

**Second, because the disagreements are the valuable part, and adopting a table destroys
them.** Three rows disagree, and each says something different:

| builder | measured here | prosper | what the difference is |
|---|---|---|---|
| `sceAgcDcbJump` | **4 dwords** | 5 | their 5th dword is their own predication flag, written into the packet after it is built. Their audit already lists the row as oversized-but-load-bearing and open |
| `sceAgcDcbDrawIndexIndirect` | **5 dwords** | 4 (published PM4: 6) | a three-way split in which no borrowed figure is the measured one |
| `sceAgcDcbWaitRegMem` | 56 bytes, 3 packets, zero arguments | 9 dwords, HIGH confidence | not yet comparable - see below |

Had the table been adopted, `DcbJump` would be 5 here, and orbistoun would have inherited a
defect the other project already knows it has. Their own audit explains the mechanism: a guest
that calls `GetSize` is self-consistent with any emitter, but a guest that **inlined the size
at compile time** never asks and cannot be corrected, so a builder emitting more dwords than
the library overruns a reservation made in good faith. That is the failure that cost them a
title. Measuring the library is the only way to be right for the second kind of guest.

## What agreement bought, since it is not nothing

Eight builders agree exactly, and one agreement is structural rather than per-row. Their audit
reconstructed a 16-dword submit epilogue from a single title's reservation behaviour and
argued its two occupants must be `AcquireMem` plus the end-of-pipe action - the reasoning that
forced their `RELEASE_MEM` from 9 dwords to 8. Measured here independently:
`sceAgcDcbAcquireMem` reserves 8 and `sceAgcCbQueueEndOfPipeAction` reserves 8. The sum is
their epilogue exactly. They inferred a pair and never measured either; both halves are now
measured, and the arithmetic closes.

## The one row that is not a contradiction yet

`sceAgcDcbWaitRegMem` looked like a flat contradiction and is better understood as
under-measured. The argument-discrimination check called it twice through one writer - all
arguments zeroed, then distinct per-argument sentinels - and got 56 bytes and then **0 bytes**.
So the builder is argument-sensitive, and the 56-byte three-packet capture is what all-zero
arguments produce, not what a title produces. A 9-dword single packet from real arguments is
entirely consistent with that. Settling it needs a capture with plausible arguments, which is a
probe request rather than something to reason out here.

Worth recording separately: obSCEne reports that result as *"the packet length depends on the
argument values"*, which its own measurement does not reach - 56-versus-0 is the shape of a
refusal, not of a shorter encoding. The knowledge entry records what was measured and notes the
gloss rather than repeating it. This is CLAUDE.md principle 3's rule about messages naming
causes, applied to a sibling's instrument.

## The scope rule that fell out of it

A `measured` entry may not carry open questions - `orbistoun-hle`'s own test says so, because
an open question against a measured function puts it back on the probe queue to re-ask
something already settled. Fourteen entries were first written with the caveat "only the size
is measured; the purpose is read off the name" as an assumption, and the gate rejected them.
The caveat is real and stays, as **prose in the entry** rather than as a question. Scope of a
claim and a worklist of unanswered questions are different things, and the schema is right to
keep them apart.

## Consequences

- 17 AGC entries became 31; 11 measured became 25.
- prosper is credited in `ACKNOWLEDGEMENTS.md` in the same change, under the rule that reading
  another project's public prose is ordinary engineering and an uncredited influence is what
  makes a provenance question unanswerable later.
- The three open divergences are candidates for a probe request, not for a decision.

## Later evidence, same day (worklog 534)

A second pass over the same sweep found ten more builders measured as **whole packets** - header and
body, not just a size - which strengthens this decision on its own terms:

- The divergence count went from three to seven, and one of the new ones **refutes a named mechanism**
  rather than a number: prosper states that `CbSetShRegisterRangeDirect` prepends a two-dword NOP
  marker `0x6875000d` mirroring the real library. The measured packet carries no marker. Had the table
  been adopted, orbistoun would have emitted a marker the library does not.
- Every divergence but one is prosper carrying **its own payload** (a private draw modifier, a
  predication slot), not misreading the hardware. On each such row the *published* PM4 size matches the
  measurement and theirs does not.
- The exception is `DcbSetIndexSize`, where they are one dword **short** of the measured 3.

None of this changes the choice; it is what the choice was made in anticipation of.
