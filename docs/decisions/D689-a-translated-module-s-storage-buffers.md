# D689 - A translated module's storage buffers are bound on the draw path, and the device asks for what the modules declare

**decided** - 2026-09-14

D552 recorded that the session requests no device features, on the reasoning that the fragment
path was designed not to need `fragmentStoresAndAtomics`: a translated module keeps its
registers in `Private` storage, so the only storage-buffer writes were the observation window
- which a fragment module skips - and a guest-memory store, which nothing emitted.

A guest's pixel shader emits one. This supersedes that part of D552.

## What the validator said

The GL cube's untextured pixel shader, translated here and offered to a driver, drew the
expected colour **and** produced five validation errors (worklog 554):

| | |
|---|---|
| `VUID-VkShaderModuleCreateInfo-pCode-08740` | the module declares `Float16`; the device never enabled `shaderFloat16` |
| the same VUID again | the module declares `Int16`; the device never enabled `shaderInt16` |
| `VUID-VkGraphicsPipelineCreateInfo-layout-07988` | the pipeline layout does not declare set 0 binding 1, which the shader uses |
| `VUID-RuntimeSpirv-NonWritable-06340` | a fragment stage writes a storage buffer without `fragmentStoresAndAtomics` |
| `VUID-vkCmdDraw-None-08600` | the pipeline statically uses set 0 and no set was bound |

The first two are not about the fragment path at all: **every** module this project emits
declares `Int16` and `Float16`, because both models declare those types in their headers. Every
compute dispatch the test suite has ever run has relied on capabilities the device was never
asked for.

## The decision

Two halves.

**The device asks for what the modules declare.** `shaderInt16`, `shaderFloat16` and
`fragmentStoresAndAtomics` are requested where the physical device offers them. The alternative
- emit modules that declare fewer capabilities - is better still and is a larger change: the
models would have to declare the 16-bit types only when something uses them, and the guest's
pixel shaders would still write memory, so the third feature would be needed regardless.

The rule D552 was protecting is kept: the capability report describes **the enabled set**, not
the physical device's, so nothing downstream can build on a claim no device backs. Its test now
asserts the report tracks what was enabled rather than asserting one fixed answer, which is the
same rule stated in a form that survives the feature being enabled.

**The draw path binds the buffers.** A descriptor set layout with the two storage buffers at
set 0, bindings 0 and 1, a pool, a set, and a bind before the draw - the shape the compute path
already had. The window sizes are parameters with a default, because they are a property of the
module being run and not of the harness.

## Why not the alternatives

*Refuse to draw a module that declares storage buffers.* It would refuse every module the
translator emits, including the ones that already draw.

*Emit a fragment variant with no storage buffers.* That is what D552 assumed, and it cannot
hold: the guest's pixel shader writes guest memory. Dropping the write would change what the
shader does - silently, since the frame looks the same.

*Leave it and rely on the driver.* It worked. That is the whole problem: an invalid pipeline
that draws correctly on one vendor's driver is a bug waiting for a different machine, and this
project's tests would not have noticed either way.

## Consequences

- `tools/validate-device.sh` runs the device tests under the Khronos validation layer. It is
  not part of `check` - CI has no device and the layer is somebody's own SDK install - so it is
  a thing a person runs after changing what the emitter declares, what the device is created
  with, or what a pipeline binds.
- One test is expected to fail validation and is skipped by name: it hands the driver a
  deliberately malformed module.
- A device that does not offer `fragmentStoresAndAtomics` cannot run a guest pixel shader that
  writes memory. The report says so; nothing pretends otherwise.
