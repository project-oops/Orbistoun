# 630. The render executor's blocker is precise: the backend trait cannot reach a shader's bytes

**2026-09-16** - mapping the one genuinely-unbuilt frontier on the critical path, so the design
decision it waits on is stated rather than rediscovered a fourth time

## What is built, and what is not

The command-stream **decode** is done and now covers indexed draws (worklog 628): a `Submission`
comes out of `pipeline::submit` carrying `modules` (translated SPIR-V, by `ResourceId`) and
`commands` (`BindShader`, `Draw`, `DrawIndexed`). The lower-level **Vulkan primitives** work and are
tested: `compute.rs::dispatch` runs a compute shader and reads back both its observation buffer and
guest memory; `framebuffer.rs` renders headless and reads pixels back.

**Nothing joins the two.** `VulkanBackend::execute` (`crates/orbistoun-gpu-vulkan/src/lib.rs:75`)
refuses every `RenderCommand` with `BackendError::Unsupported` and counts it - the honest-failure
default, naming what it cannot do rather than drawing nothing. So the executor is genuinely absent,
not merely incomplete.

## Why it cannot just be filled in

The `RenderBackend` trait is exactly three methods: `name`, `execute(&mut self, &RenderCommand)`,
`present(&mut self)`. A `BindShader` command carries a `ResourceId`, **not** the shader's bytes - the
bytes live in `Submission.modules`, which the trait gives the backend no way to receive. So a backend
cannot create a shader module even in principle through the interface it has. The same gap applies to
buffers (a `BindBuffer` names a `ResourceId`, and vertex/index data lives in guest memory the backend
is never handed).

This is the decision the executor waits on: **how does a shader module (and a buffer) reach the
backend?** Three shapes, each a real trade:

1. **A `load_module(id, spirv)` / `load_buffer(id, bytes)` pair on the trait**, called once per
   resource before the commands that name it. Smallest change; keeps `execute` command-at-a-time;
   the backend caches by `ResourceId`. A first slice could have `load_module` create a real
   `vk::ShaderModule` and validate it - Vulkan rejects malformed SPIR-V at creation, so this alone is
   a **new correctness signal for the translator's output** before any pixel is drawn.
2. **A `submit(&Submission)` method** that hands the backend the whole frame - modules, commands and
   all - and lets it order the work itself. Fewer round-trips, but it moves frame sequencing from the
   pipeline into every backend.
3. **Bytes carried in the commands themselves** (a `BindShader` holds its SPIR-V). Simplest to route,
   but it makes the command stream heavy and re-sends a shader every time it is bound, which the
   `modules` map exists precisely to avoid (worklog: "the same shader is translated once").

## Why this is flagged, not assumed

It is a **mechanism** - the kind of choice CLAUDE.md says to surface rather than settle unilaterally,
because it shapes every backend written afterwards and is hard to change once one exists (D029-D031
are the same reasoning for the backend seam that already exists). It is on the critical path (nothing
renders until it is chosen), the mature core around it is done, and off-path subsystems cannot be
exercised yet (principle 6), so it is the next real work rather than one option among many.

**Recommended direction:** option 1, starting with `load_module` creating a validated
`vk::ShaderModule`. It is the smallest step, it turns the translator's SPIR-V from "has the right
magic word" into "a real Vulkan implementation accepts it", and it commits to nothing about how draws
execute later.

## Gate state

No code changed - this is a specification of the frontier and the decision it needs. Worklog only.
