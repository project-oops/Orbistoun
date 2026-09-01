# D192 - `check` says everything before it fails, and an advisory tool cannot end the report


**decided** · 2026-08-22

Two changes to `orbistoun.sh check`, from an investigation whose premise was wrong.

### What I thought was broken, and was not

A `check` run reported a failing test and the invocation returned zero, so I concluded the
script had no `set -euo pipefail`. It has, on line 23. I had read `head -20`.

**The masking was in the invocation.** Every `./orbistoun.sh check | tail -4` returns the
status of `tail`, and a pipeline's status is its last command. The script had been reporting
correctly all along and the report was being discarded on the way out. An entire session of
"check passed, exit 0" was the exit status of `tail -4`.

Recorded rather than quietly fixed because the shape recurs: **a correct signal, destroyed
by the thing carrying it**, is indistinguishable at the far end from no signal - and it
reads as the good news rather than as an absence.

### The two real defects it did surface

**An advisory tool could end the run.** Under `set -e`, `cargo-deny` reporting a licence
finding terminated `check` before the summary of the *required* steps printed. The gate is
documented as degrading gracefully when these tools are absent; it did not degrade when one
of them was present and had an opinion. They now warn, and cannot decide the exit status.

**Fail-fast is the wrong shape here.** It is correct, and it means a tree with six problems
reports one, six times. The required steps now run in a tested context - which `set -e`
deliberately does not fire on - and their failures accumulate into a summary before a
non-zero exit. Both halves matter: a report that does not change the exit status is one no
tooling can act on, and an exit status with no report is one no person can act on.

### And one introduced while fixing it

The first edit put the accumulator into `site()` as well, where nothing checks it - so a
failed documentation build would have assembled a site from it and exited zero. The bug
being fixed, reintroduced three lines away, caught by reading the diff rather than by any
check. `site()` fails fast on purpose: a published artifact built from a failed step is
worse than no artifact.

