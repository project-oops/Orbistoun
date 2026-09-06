# D548 - The wall is an experiment, and there is no guest to run

**RETRACTED** - 2026-09-04, by [D555](D555-the-wall-was-real-and-the-record-was-twelve-days-stale.md)

> **The second half of this decision is false and the first half is misleading.**
>
> There are **seventy-one guest binaries** and there always were. `orbistoun-cli paths` reports
> the *overlay* root as `library`, and I read it as the whole answer without looking in the
> working directory. `./bin/orbistoun run` was available the entire time.
>
> And the wall is **not** an experiment. A run with zero overrides reaches `image+0xf56e09` after
> **420,611 calls** - the honest slot said 222 only because it had been unwritable since a single
> measurement was learned on 2026-08-26, which `propped_up()` counts as an override.
>
> What survives: the slot routing exists and works, and quoting a helped number as the state
> would be wrong. The rest is retracted. Left in place because the reasoning that produced it is
> the lesson.

**decided** - 2026-09-04

The standing instruction for this run is *re-derive every number*. The one number never
re-derived is the one in the instruction's own header - the wall. This is that, and it changes
what the loop has been telling itself.

## The wall is the helped slot

`compat/PPSA02664-app0.toml` keeps two records, and the loop has been quoting the second:

```toml
[status]                                    [experiment]
reach   = "entered"                         reach     = "entered"
outcome = "image+0xafc959"                  outcome   = "image+0xf56e09"
imports = 23                                imports   = 197
calls   = 222                               calls     = 420273
                                            overrides = 1
measured_on = "2026-08-23"                  measured_on = "2026-09-03"
```

`197 distinct, ~419k calls, read of 0x50 at image+0xf56e09` is the **experiment** slot: a run
with one override. The honest run of that title reaches `image+0xafc959` after 222 calls.

The slot routing is not a defect - it is D312 and D323 working exactly as designed, so a helped
run can never overwrite the honest number, and `compat list` prints the honest one. The trace
confirms the experiment's figures independently: `eboot-bin-PPSA02664-app0.json` holds 418,903
calls across 197 distinct imports.

**What was wrong was the summary I kept regenerating**, which carried the helped number as the
state without the qualifier for roughly twenty ticks. D227 already says an intervention that
moves a wall is not a diagnosis; the same applies to an intervention that moves it three orders
of magnitude and then gets quoted as where the project is.

The honest record is also **from 2026-08-23**, before nearly everything this session did - the
fixed-base heap (D513), the unrun initialiser arrays (D514, D515), the flip-pending fix (D516),
`sceKernelStat` and `sceKernelPread` (D525, D526). So it is very likely *understated* now, and
saying which way it is wrong is not the same as knowing.

## And it cannot be re-derived here

The title library holds **144 directories and 12 files, and all twelve are guest output** -
obSCEne's own `report.txt` and `boot.txt` from past runs. There is no `eboot.bin`, no `.elf`, no
guest executable of any kind on this machine.

So `./bin/orbistoun run <title>` - which this project's own notes call *the* command, the one
whose `FURTHER` verdict is the only measure of progress there is - **cannot be turned at all in
this session.** Not for the title the wall belongs to, and not for any of the twenty-two pinned
corpus payloads, whose directories exist and whose files do not.

`corpus sync` would fetch them from a public GitHub release. That is a download and an
outward-facing action, so it is not something to do unasked, and it is named here as the thing
that would unblock a run rather than done quietly.

## What this explains

Ten ticks of tests and documents, and the honest reason is not that they were the best available
work - it is that the work which *measures* progress was unavailable and nothing said so. The
loop kept picking axes it could reach: the ask list, the measured-hardware comparison, the
blocker re-derivation. All real, all bounded, none of them able to move a guest one instruction
further, because no guest could be started.

**A loop that cannot run its own measurement should say so in its first line**, not discover it
on the twenty-first tick because somebody finally read the file the number came from.

## The measured-hardware axis closes here

Five ticks, D543 to D547.

**Bought:** 22 measured facts asserted where none were - 14 refusal codes and 7 relations, plus
one divergence recorded and pinned. Two stated blockers corrected, one of them wrong about its
own mechanism and one right about its mechanism and in the wrong list. One permanent resident
removed from a work queue that exists to have none. Thirteen other blockers re-derived and found
accurate, which is worth as much as the two corrections and is easier to forget.

**Cost:** five ticks, and no movement on the wall - which, per the above, was never available to
move.

**Left:** 59 `OUTSTANDING` entries, now believed accurate, and 25 measured values whose meaning
lives in obSCEne's C source. **Every one of them needs a capture rather than code.** That makes
them obSCEne's next step and not orbistoun's, which is the honest end of an axis rather than an
exhausted one.
