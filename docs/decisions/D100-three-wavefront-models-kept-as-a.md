# D100 - Three wavefront models, kept as a differential oracle

**decided** · 2026-08-20

`Fidelity` selects how the wavefront is modelled, as a field of
`Strategy::Predicated` rather than a parameter beside it - structured reconstruction
implies the per-lane model and nothing else, so making it a field means an invalid
pairing cannot be written down.

| Level | Model | State |
|---|---|---|
| `Lane` | one invocation per lane; lanes never interact | built |
| `Wavefront` | one invocation simulates all lanes; the mask is a value | built |
| `Subgroup` | one invocation per lane; mask via subgroup ballot | stub |

**They are not a fallback ladder.** Building all three was proposed as insurance and
justified as something better: run one shader at two levels and disagreement means the
faster one has a bug, localised to that shader and bisectable to an instruction - with
no reference hardware, no console and no title. The same trick the decoder's
differential test plays, one layer up, and it already passes across four programs.

**Why the slowest first.** `Wavefront` is the simplest of the three - no subgroup
operations, no ballot, no size to negotiate - and it is the oracle the other two are
judged against. Building the hard one first would mean building it with nothing to
check it against.

**Three things inside it worth keeping.**

The **mask lives in the scalar register file** at the two indices the architecture
reserves, rather than in a variable of its own. The guest addresses `exec_lo` and
`exec_hi` as ordinary scalar registers and manipulates them as 32-bit halves, so
modelling them that way means guest code touching the mask needs no special
translation - and it sidesteps 64-bit integers entirely, on a value the guest never
treats as one.

Masked writes are a **select, not a branch**. Read the old value, compute the new one,
keep whichever the mask calls for. Same result, no merge blocks, and it keeps the level
whose purpose is obvious correctness free of the one thing that makes SPIR-V generation
hard.

The **observation layout is identical across levels** - lane zero's vector registers,
then the scalars. That is a requirement rather than a coincidence: two levels can only
be diffed if they report in the same shape.

**Configurable, but not freely.** `Auto` picks the cheapest valid level and the level
actually used is reported, because "slow" and "quietly dropped a level" look identical
otherwise. A caller may pin a level; if it is not built, that is an error naming what it
would have done. Never a silent substitution - and the reason is sharper than for the
strategy stub, since these levels differ in *correctness* rather than only speed, so a
substitution renders something wrong rather than something slow.

`Auto` resolves to `Lane` today, and the reasoning is worth recording because it is
**safety by accident**: a shader needing more than the lane model needs a cross-lane
instruction or mask arithmetic, and every such instruction is already refused as
untranslatable. The lane model therefore cannot currently be chosen for a shader it
would get wrong - not by analysis, but because the translator stops first. That stops
holding the moment those instructions are implemented, and `resolve` carries a note
saying so.

