# D708 - a wall is orbistoun's until hardware proves it the title's

**Status:** assumed
**Date:** 2026-09-19

## Context

Twice now a session has hit a wall in a real title, found something that *looked* like the
title's own incompleteness, and concluded the title was at fault - a debug build, an unfinished
code path, a phantom symbol - and stopped. Both times it was wrong: the title runs on real
hardware, so the wall was orbistoun's gap to close. PPSA02664's `CreateWorkload` is the latest:
a guest `todo:` print and an obSCEne "not a retail export" reading were taken as proof the title
was broken, until the operator said it renders on a console.

This is the honest-failure principle turned on the tools (principle 3, the "applies to the tools"
clause): "it is the title's fault" is **plausible output the measurement did not support**. A guest
log line and an unresolved import are not evidence a title is broken. And the conclusion is
seductive in a way a wrong constant is not: blaming the title moves the wall *out of orbistoun's
scope*, which ends the work. It is the emulator equivalent of a stub that returns success - it
looks like a finding and asks nothing more of you.

## Decision

**A wall is orbistoun's until hardware proves it the title's.** Concretely:

1. **The default attribution of any guest fault is orbistoun.** Orbistoun is the incomplete party -
   a work-in-progress HLE - and the title is a thing that shipped and ran on a console. A null the
   guest dereferences is a value orbistoun did not fill; a path the guest takes into a phantom is a
   path orbistoun steered it into by answering something upstream wrongly. Start there.

2. **Blaming the title's own code is a strong claim and needs strong evidence** - a hardware
   observation that the *same path fails the same way on hardware*. Absent that, "debug build",
   "incomplete", "broken", "dev scaffolding" are hypotheses, not conclusions, and must be written
   as `assumed` and left open, never used to close a wall.

3. **These are explicitly not evidence the title is broken:**
   - A guest `TODO:` / `todo:` print. Shipping titles carry benign logging markers; Unity's PS5
     backend prints them for methods that work.
   - An import orbistoun cannot resolve. That is orbistoun's missing HLE, or a firmware/SDK-era
     question - obSCEne finding a symbol "not exported on FW *n*" means the resolution differs on
     the title's platform, not that the title is defective.
   - A fault in a path whose inputs orbistoun stubbed. The stub is the suspect, not the title.

4. **Record hardware ground truth and surface it at the fault.** The compat record carries a
   `[hardware]` attestation - what a real console does with the title, and who says so (the operator,
   or an obSCEne id). When a run faults, the report prints the attribution by it: a title attested to
   render on hardware makes the fault unambiguously orbistoun's; an unknown one still defaults to
   orbistoun and says so. The ground truth is put in front of the session at the moment it is
   tempted to reach for the title's fault.

## Consequences

`orbistoun-overrides` gains `Hardware`, a title-level attestation on the compat record; the run
report frames a fault by it; PPSA02664 is recorded as rendering on hardware (operator). The
`AGENTS`/`CLAUDE.md` build principles gain the rule so it is read at the start of every session,
which is the part that actually stops the mistake - the tooling makes it visible, the principle
makes it doctrine.

This does not forbid ever finding a title genuinely at fault - a title *can* be a broken dump or a
dev build. It raises the bar to a hardware observation, which is the only thing that can honestly
carry that weight, and makes the cheap version (a log line, an unresolved symbol) count for nothing.
