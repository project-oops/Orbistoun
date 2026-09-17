# Graphics

RDNA2 PM4 command-stream decode, shader translation to SPIR-V, and the Vulkan backend that
does not present yet.

Orbistoun decodes GFX10 PM4 packets from platform graphics command buffers
(`sceAgcSubmitDcb`), translates RDNA2 Wave32 shaders into SPIR-V, and can dispatch a
translated shader on a real Vulkan device for compute. **Presentation is not implemented**:
`orbistoun-gpu-vulkan` is the only crate that knows Vulkan exists (CLAUDE.md principle 12),
and its own README states the rendering backend is still a stub — every draw and present
call is refused by name, tracked against roadmap phase 6. See
[PROJECT_STATUS.md](../PROJECT_STATUS.md) for the current, generated numbers.

There is also no CPU software renderer, current or planned: this project has deliberately
chosen not to build a second graphics backend (CLAUDE.md principle 12) or a second
execution backend, so "Vulkan or CPU fallback" is not a real choice anywhere in the tool.

---

## What exists today

- **Command-stream decode.** PM4 packet walking, register file, and pipeline assembly, in
  `orbistoun-gpu` — no dependency on a host graphics API, so nothing here can leak Vulkan
  concepts into the translator.
- **Shader translation.** RDNA2 GFX10 bytecode decode and an instruction census
  (`orbistoun-shader`), and decoded shaders to SPIR-V (`orbistoun-translate`,
  `orbistoun-spirv`), checked by executing the result on a real device and comparing
  against expected values rather than only validating structure.
- **Vulkan compute dispatch.** `orbistoun-gpu-vulkan` depends on `ash` for exactly this: a
  real device, a translated shader, and a buffer read back — the mechanical correctness
  signal the translation work is checked against.
- **Vulkan presentation.** Not yet. Every draw and present command is refused, by name, so
  a missing implementation is never mistaken for a rendering bug.

There is no GUI graphics-settings panel to describe yet, and no `run` flags for renderer
device selection or V-Sync — neither exists in `orbistoun-cli` today. When presentation
lands, this page will document the real settings surface rather than a mockup of one.
