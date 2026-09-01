# D303 - The corpus is the oracle; the probe is one member of it


**decided** · 2026-08-26 · because obSCEne will never cover every call and titles are the point

D302 made the conformance probe the acceptance gate, which was right and too narrow. A probe
grades what somebody wrote a check for, and nobody will ever write a check for every entry
point on the platform. **Title mining is the goal**: a person runs a commercial title, it dies
on a stub, and the useful question is whether orbistoun can work out what that stub owes the
guest and test the answer *there* - on the title in front of them, with no check written for
it and nobody available to write one.

It can, because a corpus is a test suite. Every title on a machine is an independent guest with
its own expectations of the same functions, and a change is measured against all of them:

| signal | what it says |
|---|---|
| distinct imports reached | how far a guest got before it stopped |
| where it faulted | whether the wall moved, and to where |
| **whether it derailed** | whether the fault is an address the guest asked for, or non-code |

The third is the one that makes reach trustworthy. `FaultSite::TOUCHED` already separates "the
guest tried to touch this" from an illegal instruction, a breakpoint or a stack overflow - and
a guest **derailed into non-code** has been broken by the change rather than helped by it, which
`Finding::Derailed` already relies on for exactly this reason. A patch that buys reach and
starts derailing something is refused.

**And the corpus scales with the person running it, not with us.** Somebody with fifty titles
has a fifty-guest regression suite for a change measured on one of them. Nothing in this
repository has to know those titles exist, hold anything about them, or grow to accommodate
them - which is the same property that makes a measurement submittable (D297).

**One datum can coincide; several agreeing is a relationship.** A wrong answer that happens to
buy reach in one guest is ordinary; one that buys reach in four and breaks none is a different
class of claim. That is the two-sentinel argument (D283) applied to guests instead of
addresses, and it is what makes reach usable as an oracle at all.

The probe does not stop being special. Where a check covers a function it says **correct**
rather than **proceeded**, which no title can. So it is scored alongside the titles and its
verdict is strictly stronger where it applies - one member of the corpus, and the only one that
grades against a spec.

