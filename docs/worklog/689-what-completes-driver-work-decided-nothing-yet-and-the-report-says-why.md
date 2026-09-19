# 689. What completes driver work, decided: nothing yet, and the report now says why

**2026-09-17** — inbox `-4f6d`: D560 made a flip complete the instant it is accepted; nobody had
made the twin decision for **submitted graphics work**, so PPSA03416's render thread blocks forever
on `UnityFTMFlipQueue` (`0 registered, 1 waits, 0 delivered`, D615) - a queue nothing can feed,
because `sceAgcDriverAddEqEvent` is unimplemented and nothing posts a completion. 4f6d wanted the
decision, then either the completion posted, or the honest "nothing posts yet, and here is why."

## The decision (D705): nothing posts yet

`docs/decisions/D705`, status `assumed`. **Driver-work completion is not posted**, and
`sceAgcDriverAddEqEvent` stays unimplemented. It is **not** the flip twin: a flip's whole
post-acceptance job is scanout, which orbistoun has no model for and never will, so acceptance *is*
completion with nothing skipped; a submission's post-acceptance job is **GPU execution**, which
orbistoun does not do yet but is building (roadmap phase 6, `-36c0`). Posting a completion for work
that never ran is the plausible-output failure principle 3 forbids - and three things already in the
tree agree:

- **D613 made the wait a real wait on purpose**, calling the honest permanent starvation better than a
  busy loop. Faking completion trades it back for a loop churning frames into a buffer nothing renders.
- The `sceAgcDriverAddEqEvent` note already calls the halfway measure wrong: *"a queue that is
  registered and never posted to is the same wait, arrived at more slowly."*
- An intervention that moves a wall needs a second observation (D226/D227); firing completion has
  none, and what the console posts to such a queue is obSCEne `-3423`, unresolved.

When execution lands, completion posts from the real thing - orbistoun processes a submission
synchronously, so the event fires the instant the executed submit returns, the same shape D560 gives
flips but earned by work that ran. D705 is `assumed` so it retires the moment execution or 3423
arrives.

## The report now names the reason

`equeue_summary` (`orbistoun-kernel/src/lib.rs`): the starved-queue verdict grew from *"waited on,
never delivered"* to name why - *"nothing produces its events; a graphics queue waits on driver-work
completion, not posted yet (D705)"*. A reader of a PPSA03416 report learns the cause from the line
rather than reaching for D615. This is a deterministic string on the existing `waited > 0 && delivered
== 0` branch, so no run is needed to confirm it renders; PPSA03416's `UnityFTMFlipQueue` line is where
it shows. The `sceAgcDriverAddEqEvent` knowledge note was updated from "nobody has answered this" to
point at D705.

## Gate state

`orbistoun-kernel` tests pass (no verdict-string test to update); `clippy -p orbistoun-kernel
--all-targets -D warnings` clean; decisions index regenerated (715 entries, D705 listed) and
`check-decisions` exit 0; `./bin/orbistoun prose` exit 0; `cargo fmt --check` clean; knowledge-audit 27
pass; identity scan clean. No commit.
