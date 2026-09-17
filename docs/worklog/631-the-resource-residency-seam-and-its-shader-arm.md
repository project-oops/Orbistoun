# 631. The resource-residency seam, and its shader arm: a translated module now loads on a real device

**2026-09-16** - the render executor's first slice, built to the scalable shape agreed in D701:
the backend can be handed a resource to make resident, and the Vulkan shader arm turns the
translator's SPIR-V into a module a driver accepts

## What this closes

Worklog 630 mapped the executor's blocker precisely: the `RenderBackend` trait had no way to receive
a shader's bytes, so `VulkanBackend::execute` refused everything and the working Vulkan primitives
sat unwired. The design question - how a resource reaches the backend - was surfaced rather than
settled unilaterally. It is now decided (D701) and its first arm built.

## The mechanism (D701)

`RenderBackend` gains `ensure_resident(id, Resource)` and a default-keep `release(id)`. `Resource` is
an enum with one arm today, `Shader(&[u32])`, that grows to `Buffer`/`Texture` as the decode side
learns to extract them - a backend matches it exhaustively, so a new kind cannot be silently dropped.
Residency is **idempotent and content-keyed**: the `ResourceId` the frontend already mints is a
content hash, so a resource the backend holds is reused ("upload once", mirroring "translate once"),
and a guest that rewrites a resource changes its id and gets a new one - invalidation for free.

The seam keeps guest memory on the frontend side (it has `GuestMemory`; it hashes and extracts the
bytes) and hands the backend plain data, so `orbistoun-gpu` still names no graphics API - the boundary
`cargo` enforces (D029, principle 12). Why not the alternatives - `submit(&Submission)`,
bytes-in-commands, a guest-memory handle to the backend - is in D701.

## The shader arm

`VulkanBackend` now holds `BTreeMap<ResourceId, vk::ShaderModule>`, acquires the device lazily (via
the existing `compute::session()`), and on `ensure_resident(Shader)` creates the module unless the id
is already resident, destroying all of them on drop. **Because `vkCreateShaderModule` accepts it, the
translator's output is validated as a module a Vulkan implementation takes** - a stronger signal than
"the words begin with `0x07230203`". Draws still refuse honestly (D010); execution is the next arm.

`RecordingBackend` records residency (a `resident()` list, cleared with the frame), so the seam is
exercised with no GPU at all.

## Made to fail

- `a_translated_shader_is_made_resident_once_and_reused` (gpu-vulkan) - device-gated like the compute
  tests (skips where there is no device; here there is one). A `minimal_compute_module` from
  `orbistoun-spirv` is made resident: the driver accepts it, a second `ensure` of the same id is a
  cache hit (`resident_shaders()` stays 1), and a different id is a second module. Fails against a
  version that re-creates on every call, or one that does not create at all.
- `resources_are_made_resident_in_order_and_cleared_with_the_frame` (gpu-gpu, no device) - residency
  is recorded in reference order, is not a command, and clears with the frame; `release` is a no-op
  by default, so a never-evicting backend stays correct.

## What is next, under the same seam

The thin driver (`ensure_resident` per resource → `execute` per command → `present`) lands with the
first executing command; the `Buffer`/`Texture` arms land as the decode side extracts them; pipeline
assembly and eviction stay the backend's internal concern (D701). None of it changes the interface.

## Gate state

`cargo test -p orbistoun-gpu -p orbistoun-gpu-vulkan` green (2 new tests, one device-gated);
`cargo clippy` on both `--all-targets` clean; fmt clean. D701 recorded (status `decided`).
