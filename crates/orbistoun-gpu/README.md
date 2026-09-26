# orbistoun-gpu

GPU translation: vendor command streams to Vulkan, vendor shader bytecode to SPIR-V.

It holds the graphics driver library entry points, the command-packet walker, the register
file the packets write into, the pipeline state they assemble, and the `RenderBackend`
contract a host implementation fills in. It creates no Vulkan device: the crate has no
dependency on any host graphics API (CLAUDE.md, *Contracts at guest semantics*), and
[orbistoun-gpu-vulkan](../orbistoun-gpu-vulkan/) is the backend. Shader work goes through
[orbistoun-shader](../orbistoun-shader/) and [orbistoun-translate](../orbistoun-translate/),
which emit through [orbistoun-spirv](../orbistoun-spirv/).

## Two translations

- **Command stream**: vendor packet buffers to backend commands. Structural and high-volume;
  this is where hardware features with no Vulkan equivalent live.
- **Shaders**: vendor shader bytecode to SPIR-V. Pattern-heavy and differentially verifiable.

The layer has a cheap correctness oracle: render a frame, diff the framebuffer against a
reference, get a number. See [docs/TESTING.md](../../docs/TESTING.md).

## Unified memory

The hardware has one coherent pool shared by CPU and GPU; guests map GPU-visible memory and
write it from the CPU with no explicit transfer. A discrete host GPU has no equivalent, so
this layer detects those writes and synthesises the transfers. The gap is semantic, not a
performance matter: ignoring it produces frames that are subtly wrong rather than obviously
broken.
