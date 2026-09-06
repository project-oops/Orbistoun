# D507 - A second capture settles the third query field, and demotes thirty measurements

**measured** - 2026-09-03 (`live_launch.txt`, the same build shape as the capture before it)

A new hardware capture arrived: same `meta`, same `build|dev|module`, same context, and a better
run - **144 checks passed where the previous one managed 122**, with three fewer failures and
thirteen fewer partials.

```text
measurements   224 -> 227        constant   200 -> 172
CLAIMED         73 -> 76         OPAQUE      58 -> 28        OUTSTANDING   69 -> 68
```

## The third field of the query structure is the memory type

`orbistoun-kernel` has carried this beside that field:

> What `3` denotes - a type, or some state - is still open, and **one run distinguishes them:
> allocate with several types and query each back.**

That run has now been taken. One 16 KiB page allocated with each of `WB_ONION` (0),
`WC_GARLIC` (3) and `WB_GARLIC` (10), and the field read back for each:

| asked | read back |
|---|---|
| 0 | `0x0` |
| 3 | `0x3` |
| 10 | `0xa` |

**Every one is the type it asked for.** Three distinct answers also rule out the other reading
obSCEne's own comment named - *"if it is the same value for every type, it is state and the type
is somewhere this does not read."*

So the model orbistoun already had was right before it could be checked, and the three
measurements are claimed by
`the_third_query_field_is_the_memory_type_the_allocation_asked_for`. The test allocates and then
queries rather than reading a region orbistoun built itself, because the latter would pass on
the model alone. Watched failing by writing a constant `1` into the field.

**This is only measurable because the run got direct memory.** The previous capture recorded
`alloc-refused` for all three types - the arbitrator granting that process nothing - so both
sets of records are real and neither supersedes the other: one says what happens when the
allocation is refused, the other what the field holds when it is not.

## The poison answered D497, and the answer cannot live on that measurement

Yesterday's probe change (obSCEne's D303) initialised the encoder path-probe's out-parameter to
`0xC7C7C7C7` instead of zero, because a zero could not be told from a field the platform never
writes. All twenty-four paths now report **`0xc7c7c7c7`**: **the platform does not write that
out-parameter on a refused load.**

The measurement id cannot carry that finding. The two captures disagree - `0x0` then
`0xc7c7c7c7` - so the generator marks all twenty-four **not constant**, and a varying
measurement must never be asserted. It is right to: the disagreement is between two *probes*,
not two consoles. But the consequence is that the twenty-four leave `OPAQUE` and the result
would vanish with them.

Recorded where a non-constant-but-informative result belongs: an **edge case** on
`sceKernelLoadStartModule` in the knowledge base, with the poison, the paths and the code in its
own text.

**Not as `known_by = measured`, and a guard said so.** Setting that tripped
`a_question_is_never_recorded_against_something_already_measured`: *"measured means hardware
answered it. An entry claiming that and listing an open question is contradicting itself, and
the queue would send a probe to re-ask something already settled."* This entry has plenty of
open questions - the whole parked module thread - so one measured fact about it does not make
the entry measured. `known_by` stays `guest-observed`, which is what its siblings
`sceKernelGetModuleList`, `GetModuleInfo` and `Dlsym` carry for the same shape: measured edge
cases, open assumptions.

The distinction is worth keeping: `known_by` describes the entry, an edge case describes one
behaviour, and a measurement that arrives as an edge case must not be promoted to the entry.

## Six of D502's ten were confirmed by the mechanism rather than by my judgement

D502 moved ten measurements out of the work queue on the argument that they were *"a moment on
one machine"* or *"a rate on one machine"*. A second capture now disagrees with the first on
exactly six of them - the three `clocks-advance` absolutes and three `timer-ratio` deltas - so
the generator marks them non-constant on its own. The argument was right and is no longer
needed.

**And it corrects the one D502 hedged on.** `timer-ratio:tsc_hz_calibrated:hz` was kept
outstanding as *"claimable in principle - it agrees with the reported frequency to eight
figures"*. Across the two captures it reads `0x3d554b40` and `0x5f2476c6`: **1.03 GHz against
1.60 GHz, a 55% swing.** `nothing_claims_a_measurement_that_cannot_be_claimed` refused it by
name, which is the guard doing what the hedge could not.

## What a capture costs to absorb, which is the reusable part

`cargo run -p orbistoun-gen -- measurements --records <dir>` regenerates the table from every
capture in a directory and marks a measurement constant only when every run that took it
agreed. Adding one capture then produces a precise work list from two gates: what is newly
measured and unaccounted for, and what is named in a list but no longer constant. Both fired,
both named the exact ids, and the work was following them.

Worth stating because the alternative - reading a 11,002-line capture by eye - is how a
finding like the third field stays open for another month.
