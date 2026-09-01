# D098 - Predicated translation first; structured reconstruction stubbed loudly

**decided** · 2026-08-19

The execution-model question, settled. Two new crates: `orbistoun-spirv` builds SPIR-V
modules and knows nothing about the guest; `orbistoun-translate` maps one onto the
other.

**The mismatch.** The guest runs 64 lanes in lockstep under an explicit execution mask.
There is no `if` in its machine code - a branch is mask arithmetic followed by a jump
taken when no lane survives, so structure is *implied*. SPIR-V describes one invocation
and demands **structured** control flow: explicit merge blocks, a reducible graph, the
hardware handling divergence.

**Chosen: predicated.** One invocation per lane, the execution mask as a per-invocation
boolean, and the flat instruction stream expressed as a loop around a switch on the
program counter - the standard way to express an unstructured graph in a structured
language.

It is slow, and the output is unreadable. It is also **always correct**, including for
control flow that cannot be structured at all, and it is mechanical enough to build and
verify one instruction at a time.

**Rejected for now: structured reconstruction.** Faster, and readable - which genuinely
matters when the intended reader is meant to reason about the output. But it is a
decompilation problem, idiom recognition is semantic rather than syntactic, and getting
one wrong renders subtly incorrect output rather than failing. It also needs the
predicated path as a fallback regardless, since irreducible control flow always exists.

**Rejected outright: simulating a whole wavefront per invocation.** Exactly correct,
and it serialises the machine's parallelism into a single invocation. Named here only
to fix the axis.

**Three reasons for the slow one first**, in increasing weight: performance is not a
goal yet; it reaches a rendered frame soonest, and a rendered frame is what turns on
framebuffer diffing - the only cheap correctness oracle this layer has; and **it becomes
the reference implementation for the other one**. Once predicated translation renders
correctly, a structured translator can be checked against it by comparing framebuffers.
Without that there is no way to validate the structured path at all.

That is the same shape as `RecordingBackend` and the decoder's differential test: build
the obviously-correct thing, then use it as the oracle for the clever one.

**Structured is stubbed loudly, and that is the point.** Asking for it returns an error
naming what it would do and stating that it is *not* falling back. A silent fallback
would make it appear implemented - and since its only advantage is speed, the symptom
would be unexplained slowness rather than a missing feature, which is a bug report
nobody can act on. Principle 3 applied to a whole subsystem.

`translate` refuses three things rather than approximating them: an unbuilt strategy, an
untrustworthy decode, and an instruction whose operand layout is unknown. In each case
emitting *something* would mean inventing behaviour the guest never asked for, and a
shader that renders the wrong thing is far harder to diagnose than one that does not
render.

