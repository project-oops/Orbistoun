# D133 - The subgroup level is blocked on hardware, not on effort


**Status:** assumed

`Fidelity::Subgroup` stays a stub, and the reason is now measured rather than assumed.

It materialises the guest's execution mask with a subgroup ballot, which is correct only
when the hardware's subgroup is as wide as the guest's wavefront - sixty-four lanes.
Neither device this project can reach is:

| Device | Subgroup size |
|---|---|
| RTX 5070 Ti | 32 |
| lavapipe (software, in the build VM) | 8 |

So the level cannot execute here at all, and **the differential oracle that would verify
it does not exist on this hardware**: comparing it against the wavefront model on the same
shader needs both halves to run, and one of them would be refused.

Building it anyway would produce a model checked by `spirv-val` and nothing else -
structurally valid SPIR-V that has never executed - in a subsystem where every other
component is verified against a real device. That is the shape of output this project
refuses everywhere else, and it does not become acceptable because the instruction set is
interesting.

**What would unblock it:** any AMD GCN part, or an RDNA part in its sixty-four-wide mode.
`cargo run --example subgroup-size -p orbistoun-gpu-vulkan` answers the question on any
machine in about a second.

**A design question this raises, deliberately not decided here.** Most hardware is
thirty-two wide - NVIDIA always, RDNA usually - so a level that requires sixty-four is of
limited use even once it can be tested. A variant where each invocation carries *two*
lanes, taking two ballots to assemble the sixty-four-bit mask, would run on the hardware
that actually exists and would therefore have an oracle. That is a different model from
the one D098 describes, and inventing a fidelity level unprompted is the kind of concept
that warrants asking rather than assuming.

**The correction worth recording:** the roadmap called this the last item with a real
oracle and no dependency. That was wrong, and it was wrong in the direction that matters -
it would have led to building the one thing here that could not be checked. The check
that caught it took one example and under a minute, and it was run because the claim was
load-bearing rather than because anything looked suspect.


