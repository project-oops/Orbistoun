# D705 - driver-work completion is not posted until execution lands - the D560 flip twin, declined

**Status:** assumed
**Date:** 2026-09-17

## The question

A guest hands a draw command buffer to the graphics driver (`sceAgcDriverSubmitDcb`, worklog 687),
registers a completion event on a queue (`sceAgcDriverAddEqEvent`), and then blocks on that queue
(`sceKernelWaitEqueue`) until the GPU signals the work is done. In an emulator that does not execute
the work, **what stands in for its retirement?** D560 answered the equivalent for flips - a flip
completes the instant it is accepted, because there is no scanout - and nobody had answered it here
(the note on `sceAgcDriverAddEqEvent` in `libSceAgcDriver.toml`). The measured symptom is
`docs/decisions/D615`: PPSA03416's render thread blocks on `UnityFTMFlipQueue`
(`0 registered, 1 waits, 0 delivered`), a queue nothing can feed.

## The decision

**Nothing posts driver-work completion yet, and `sceAgcDriverAddEqEvent` stays unimplemented.** The
run report names the reason a driver queue is starved, so a reader learns it from the report rather
than from D615.

## Why this is not the flip twin

D560's flip is honest because a flip's **entire** post-acceptance job is scanout, and orbistoun has
no model for scanout and never will - so acceptance *is* completion, with nothing skipped. A
submission's post-acceptance job is **GPU execution**: the shaders and draws the command buffer
carries, run against real surfaces. Orbistoun does not do that yet, and - unlike scanout - it is
**building** it (roadmap phase 6, `REQ-...36c0`). Posting a completion for work that never executed
is therefore not the same honest instant-completion; it is a claim that retired work produced a
result it did not.

Three things already in the tree point the same way:

- **D613 made the wait a real wait on purpose.** It replaced a busy loop with a genuine block,
  calling the honest permanent starvation better than a spin. Posting a completion nothing produced
  would trade that honest wall back for a busy loop that churns frame after frame into a buffer
  orbistoun never renders - the plausible-output failure principle 3 forbids, one layer up.
- **The `sceAgcDriverAddEqEvent` note already calls the halfway measure wrong**: *"a queue that is
  registered and never posted to is the same wait, arrived at more slowly."* Registering without a
  producer buys nothing; a producer that lies buys a regression.
- **An intervention that moves a wall needs a second observation** (D226/D227). Firing the completion
  would move the wall with no measurement of what the guest then does, and none of what the console
  itself posts to such a queue - `obSCEne REQ-...3423`, unresolved.

## What this is not

It is **not** a claim that driver work can never complete here. When execution lands (36c0),
completion posts from the real thing: orbistoun processes a submission synchronously (there is no
asynchronous GPU), so the event fires the instant the executed submit returns - the same shape D560
gives flips, but earned by work that ran. Until then the honest state is a named starvation, and this
decision is `assumed` precisely so it retires the moment either execution or the `3423` measurement
arrives.

## Consequence

`sceAgcDriverAddEqEvent` is left to the loud-stub policy (refused by name), not added to
`agc_driver.rs`'s `implementations()`. `equeue_summary` (`orbistoun-kernel`) names the reason a
waited-never-delivered queue is starved, pointing at this decision, so the D615 line stops being a
symptom a reader has to diagnose.
