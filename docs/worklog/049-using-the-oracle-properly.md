# Using the oracle properly


The two fidelity models are now compared on generated programs rather than on a handful
of hand-written ones. `tests/agreement.rs` builds seeded sequences from the instructions
both levels accept, runs each at both levels on a real device, and asserts identical
registers and identical memory. Forty-eight programs, all forty-eight compared.

This is the oracle D100 kept two models for, used as the property it always was. The
existing `the_models_agree_about_…` tests each cover the instruction they were written
for; a generator covers sequences nobody would think to write, which is where a
disagreement is most likely to hide.

**Verified by breaking a model**: indexing the wavefront scalar file one register high is
caught on the first seed. That check matters more than the test passing - a property test
that has only ever passed cannot be told apart from one that asserts nothing, and this
project has found exactly that in its own tests more than once.

**What it is blind to, and this is worth being explicit about.** Both models dispatch
through the same `model::instruction`. An instruction translated wrongly *once* is wrong
in both, and they agree perfectly. This finds mistakes in what differs - register files,
masking, lane loops - and nothing else. `execute.rs` covers the other half by asserting
against values worked out by hand.

**The correction that prompted it.** The previous entry said instruction breadth was
exhausted and concluded everything else waits on a capture. The first half was true and
the second did not follow. Breadth is one axis; depth on what already exists is another,
and the oracle for it was already built and under-used. Being blocked on inputs is not
the same as having nothing worth doing, and it is worth being suspicious of the reasoning
that turns one into the other.

