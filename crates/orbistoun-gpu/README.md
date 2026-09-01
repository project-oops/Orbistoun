# orbistoun-gpu

GPU translation - vendor command streams to Vulkan, vendor shader bytecode to SPIR-V.

**Models:** declarations for the graphics driver library submission entry points, the
command-packet walker, the register file the packets write into, the pipeline state they
assemble, and the `RenderBackend` contract a host implementation fills in.

**Deliberately fakes:** the drawing. No Vulkan device is created here - by construction,
since this crate has no dependency on any host graphics API (principle 12).

**Design note.** The hardest remaining problem in hardware emulation, and the only
place in this codebase with a genuinely cheap correctness oracle: render a frame,
diff the framebuffer against a reference, get a number. That makes it the best
target for tooling and automation - see `docs/TESTING.md`.

Two quite different jobs live here. Command-stream translation is structural and
high-volume, and it is where hardware features with no Vulkan equivalent hurt.
Shader translation is pattern-heavy and differentially verifiable.

**The unified-memory gap is semantic, not performance.** The hardware has one
coherent pool shared by CPU and GPU; guests map GPU-visible memory and write it from
the CPU with no explicit transfer. A discrete PC GPU across PCIe has no equivalent,
so this layer must detect those writes and synthesise the transfers. Pretending
otherwise produces frames that are subtly wrong rather than obviously broken.

**Status:** the packet walker, register file, and pipeline assembly exist and are tested;
no guest has reached a submission yet, so none of it has run against real material.
`docs/ROADMAP.md` phase 6, built ahead of the spine alongside
[orbistoun-shader](../orbistoun-shader/), [orbistoun-translate](../orbistoun-translate/)
and [orbistoun-spirv](../orbistoun-spirv/).
