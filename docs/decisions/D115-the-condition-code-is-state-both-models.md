# D115 - The condition code is state both models hold


**Status:** decided (2026-08-21) - the case it deferred has arrived

The scalar condition code is a private word holding 0 or 1, in the per-lane model as well
as the wavefront one. `SOPC` compares write it; `s_cbranch_scc0` and `s_cbranch_scc1`
read it.

Unlike a lane mask it is **not per-lane** - it is one bit for the whole wavefront - so
the per-lane model can represent it exactly, and a shader using only the condition code
must not be routed to the wavefront model. Doing so would be correct and sixty-four times
slower for nothing, which is why `touches_mask` covers only the four mask branches and
not the two condition-code ones.

**The compares are signed.** `SLESS_THAN`, not `ULESS_THAN`. The two agree on every pair
of non-negative values and disagree wherever one is negative, so a shader comparing only
small positive numbers works either way and a shader comparing against -1 takes the wrong
branch every time. There is a test whose only job is that distinction.

### The deferred case arrived, and it was already in the corpus

This was to be re-checked "once a real shader mixes condition-code and mask branches in one
block". One does, and it is a compiled fixture rather than something written to make the
point: `control` contains one condition-code branch and two mask branches, and has since it
was generated.

The mask branches decide the fidelity, which is right. A mask cannot be represented without
a model that has one; the condition code can be represented in either. So mixing does not
weaken the rule that a condition-code-only shader stays on the fast model - it means
something else in the same shader outvoted it.

**The claim itself was untested.** What had a test was the *routing* - that a shader using
only the condition code is not pushed onto the slow model. That the condition code then
behaves identically when a shader is on the slow model for some other reason - which is
what "state both models hold" asserts - had nothing asking. It does now: the same program
runs at both fidelities and the results are compared word for word.

Both halves hold.

