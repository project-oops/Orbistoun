# D289 - The plan gets a runner, and an out-parameter finding follows itself through


**decided** · 2026-08-26 · `turn::plan` had no consumers outside its own tests

`orbistoun-propose::turn` maps every finding a report can name to a next step, and nothing
anywhere called it. Four step kinds were produced and none was ever taken: the dispatcher was
a **planner**, and the sentence "step 17 is now partly mechanical" was true only of code
somebody ran by hand.

So `turn::take` runs one step and `turn::turn` runs a plan, returning what each produced.
Three things about it are deliberate.

**The trait grows a second method rather than the runner taking a concrete type.** `Trial`
had only `run`, so anything touching an axis needed `GuestTrial` - and a dispatcher that can
only be exercised by booting a commercial title is a dispatcher with no unit tests. `spawn`
joins the trait, the mock implements it, and the runner is testable against a guest that
exists only in memory.

**A step it cannot take says which kind of cannot.** `Person` is a refusal with a reason.
`NameAHash` is *automatic* and still not runnable here, because it needs a model and a
local runtime that the sweep has no business starting. Those are different facts and
collapsing them would report the naming loop as a policy refusal, which it is not.

**An `OutParameter` finding follows itself through.** The sweep concludes "arg0, offset
`+0xfffe0`, when the call answers zero" - and the next question is entirely determined by
that answer: reserve a region, plant its base, see whether the guest goes further. It is one
more run with no decision in it, so it happens rather than being printed as a suggestion.

That last step is the one that changes what the loop *is*. Everything before it measures the
shape of a gap; this one **satisfies** the contract it measured and asks the guest whether
that was enough. The answer is `FURTHER` or it is not, and either way nothing has been
guessed - what was planted came out of the sweep, not out of anybody knowing what the
function is for (D284).

