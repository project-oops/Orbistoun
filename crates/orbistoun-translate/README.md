# orbistoun-translate

Decoded guest shaders into SPIR-V.

**Models:** the control-flow strategies, the wavefront fidelity models, per-instruction
translation, and block reconstruction.

**Deliberately fakes:** nothing. An instruction it cannot translate is refused by name.

**Design note.** The guest architecture runs sixty-four lanes in lockstep under an explicit
execution mask. There is no `if` in its machine code - a branch is mask arithmetic followed
by a jump taken when no lane survives. Structure is *implied*.

SPIR-V is the opposite: it describes one invocation and demands **structured** control flow
- explicit merge blocks forming a reducible graph, with the hardware handling divergence.
Bridging that is what this crate is.

**Two axes, not one.** `Strategy` chooses how control flow is expressed; `Fidelity` chooses
how the wavefront is modelled. They were conflated at first. Fidelity is a *field* of
`Strategy::Predicated` rather than a parameter beside it, because the combinations are not
free - making it a field means an invalid pairing cannot be written down, which beats
rejecting one at run time.

**Translation is executed, not asserted.** Per-instruction behaviour is checked by running
the result and comparing, not by inspecting the emitted structure -
[orbistoun-gpu-vulkan](../orbistoun-gpu-vulkan/)'s compute path dispatches a translated
shader with known inputs and reads the buffer back. Valid SPIR-V that computes the wrong
thing is the failure this whole layer exists to avoid.

**Status:** the largest test suite in the workspace, and no guest has reached it.
