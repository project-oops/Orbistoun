# D312 - A run helped by named overrides was recorded as an honest measurement


**decided** · 2026-08-27 · found by asking what carries a finding back into the repository

`compat record` refuses a run whose stubs were reporting success, because such a run reaches
further by construction and would become a best-ever entry no honest run could beat (D182).
The guard:

```rust
!self.default_return.is_empty() && self.default_return != "unimplemented"
```

It reads the **global default**. `Learned::policy()` deliberately leaves that at
`unimplemented` and puts its answers in per-function overrides (D296), so a measured policy
reports `false` and records clean. The feature built to carry the loop's findings drove
straight through the guard written to stop exactly this.

`Conditions` already counted the overrides. Nothing read the count, and `policy_summary`'s
own doc says why it exists: *"loosening the default is the single change that improves every
number in a report while implementing nothing"* - which is true of answering one function by
name, at a smaller scale.

**The refusal was the wrong repair.** Turning a propped-up run away means a person has to
pass `--force` on every measured policy, and throws away the number that says whether a patch
is worth pursuing. The refusal only existed because a single best-ever entry could not hold
both kinds of result.

So the record holds two: `[status]` for what the emulator does as it stands, `[experiment]`
for the furthest it got while being helped. A run is routed to its own slot, compared only
against that slot, and **never refused on policy grounds**. `--force` survives for one case -
overwriting a better entry within a slot.

Comparability is a *fact* rather than a count: propped or not. Two experiments differing by
one override are not different in kind, and treating them as incomparable would recreate the
refusal in another form.

**This is what "without a person" costs.** Every guard here that refuses rather than records
is a place the loop stops and waits. The honest version of a guard is usually a field, not a
gate - the run happened, the number is real, and what a reader needs is to know which
question it answers.

