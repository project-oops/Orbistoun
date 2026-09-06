# D557 - A measured answer is not a prop, and a region always was one

**Status:** assumed
**Date:** 2026-09-04

## The question

`Status::propped_up` decides whether a run measured the emulator as it stands or was helped along,
and a title recorded as helped is kept out of the honest slot and ranked separately. It answered
the question as *"the default was loosened, or any function was answered by name at all"*.

D555 found the consequence: PPSA02664's honest record had been **unreachable since 2026-08-26**,
because one learned fact was loaded and any override at all made the run an experiment. The
question put to the operator was whether a learned measurement should count. The answer was that
anything moving the work forward is worth doing.

## The decision

**Draw the line at provenance, not at existence.** `Oracle` already grades how a fact was
established, and the file the loop writes already records it - `known = "guest-observed"` sits in
today's only learned entry. That grade was being **thrown away** at one line, `Learned::policy()`,
so every derived answer arrived downstream indistinguishable from one somebody typed into a file.

An answer is now knowledge or a prop by where it came from:

| Provenance | Counts as | Why |
|---|---|---|
| published, differential, measured | knowledge | nobody tuned it to move a guest |
| guest-observed | prop | somebody tried answers until the guest proceeded |
| assumed | prop | it rests on nothing |

`guest-observed` on the prop side is the part worth arguing with, and it is deliberate: the guest
proceeding is one bit of *consistency*, not correctness, and a run resting on one is an
experiment. It is also why this change moves **no number in the corpus today** - the only learned
entry there is guest-observed, so PPSA02664 is still an experiment when it is loaded. What is
fixed is the mechanism; the first hardware-measured answer will land in the honest slot instead of
being quietly demoted.

## The hole found on the way

**A region was never counted at all.** `Status.overrides` came from `policy.overrides.len()`, and
a region - which writes a base into guest memory behind a byte count nothing measured, as today's
entry says in its own `assumes` - lives in a different map. A policy that wrote guest memory and
answered nothing reported zero overrides and read as an unassisted run.

The count is now of **symbols** decided for, answers and regions together, once each. That is why
this change cannot only ever loosen: a run can move from experiment to honest by being *measured*,
and it can move the other way by being caught writing.

## Where the provenance lives

Beside the answers, in `StubPolicy::known`, rather than inside them - the answer is what the guest
observes and the provenance is what a report observes, and folding them together would put a field
in the guest's path that nothing in the guest's path reads.

Two maps can drift, so **a name with no provenance reads as assumed**. Drift can therefore only
ever call a run *less* honest than it was; the opposite default would let a policy become honest by
losing information, which is the failure worth designing against. A guard asserts it.

## What this does not establish

**That any label is true.** A measurement's provenance is its own account of itself, and nothing
here checks it - a mislabelled entry is indistinguishable from a correct one by construction. What
the design buys is that the label is set from the measurement that produced the answer rather than
by whoever writes the policy file, so mislabelling takes a lie rather than an omission.

**Nor that guest-observed belongs on the prop side.** That is a judgement about what "propped up"
should mean, recorded as `assumed` for exactly that reason. If a hardware sweep later shows the
loop's guest-observed answers agreeing with the target, the grade to change is the entry's, not
this line.

## Status of the old rule

Not loosened away. `propped_up` fires on a strictly smaller set than before, so the guard that
caught the D312 case asserts it **still** catches it: every answer resting on nothing measured
props a run up exactly as it used to, and the region case it never saw now props one up too.
