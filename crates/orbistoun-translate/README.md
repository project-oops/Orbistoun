# orbistoun-translate

Decoded guest shaders into SPIR-V.

It holds the control-flow strategies, the wavefront fidelity models, per-instruction
translation, and block reconstruction. An instruction it cannot translate is refused by name.
It reads [orbistoun-shader](../orbistoun-shader/)'s decoded instructions and emits through
[orbistoun-spirv](../orbistoun-spirv/).

## Implied structure to structured control flow

The guest architecture runs sixty-four lanes in lockstep under an explicit execution mask.
Its machine code has no `if`: a branch is mask arithmetic followed by a jump taken when no
lane survives, so structure is implied. SPIR-V describes one invocation and demands structured
control flow - explicit merge blocks forming a reducible graph, with the hardware handling
divergence. Bridging the two is this crate.

## Rules

- **Two axes.** `Strategy` chooses how control flow is expressed; `Fidelity` chooses how the
  wavefront is modelled. Fidelity is a field of `Strategy::Predicated` rather than a parameter
  beside it, because the combinations are not free: an invalid pairing cannot be written down.
- **Translation is executed, not asserted.** Per-instruction behaviour is checked by running
  the result and comparing, not by inspecting the emitted structure:
  [orbistoun-gpu-vulkan](../orbistoun-gpu-vulkan/)'s compute path dispatches a translated
  shader with known inputs and reads the buffer back. Valid SPIR-V that computes the wrong
  thing is the failure this layer exists to avoid.
