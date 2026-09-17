# D701 - ensure_resident is the backend resource-residency mechanism, generalizing modules to all resource kinds

**Status:** decided
**Date:** 2026-09-16

## The choice

The render backend needed a way to receive a shader's bytes: a `BindShader` command carries a
`ResourceId`, but the SPIR-V lives in `Submission.modules`, and the `RenderBackend` trait
(`name`/`execute`/`present`) gave a backend no path to it (worklog 630). The choice is the shape of
that mechanism, and it is a **resource-residency** call, not a shader-delivery one:

```rust
enum Resource<'a> { Shader(&'a [u32]) }              // grows: Buffer, Texture, …

trait RenderBackend {
    fn ensure_resident(&mut self, id: ResourceId, resource: Resource<'_>) -> Result<(), BackendError>;
    fn execute(&mut self, command: &RenderCommand) -> Result<(), BackendError>;
    fn present(&mut self) -> Result<(), BackendError>;
    fn release(&mut self, id: ResourceId) {}          // default: keep it
}
```

`ensure_resident` is **idempotent**, keyed by the content-addressed `ResourceId` the frontend
already mints (`content_hash(bytes) ^ stage ^ window`): a backend that already holds an id reuses its
host object and ignores the data. It is called for every resource a frame references, before the
commands that name it.

## Why residency, and not `load_module`

Worklog 630's first framing was a shader-only `load_module`. That is the wrong grain: buffers and
textures reach the backend the same way, and a shader-only method would be rebuilt twice. `Resource`
is an enum with one arm today (`Shader`) that grows exactly as `RenderCommand` does - a backend
matches it exhaustively, so a new kind cannot be silently dropped. The method is named for the
general mechanism from the first commit; the *coverage* grows arm by arm.

## Why the three alternatives were not taken

- **`submit(&Submission)`** (hand the whole frame to the backend) moves frame sequencing into every
  backend for no gain - the frontend already decoded the order. Keeping `execute` command-at-a-time
  and the frame loop in a thin driver keeps backends as simple recorders.
- **Bytes carried in the commands** (`BindShader` holds its SPIR-V) re-sends a shader on every bind
  and defeats the content cache that "translate once" exists to protect (the pipeline module doc).
- **A guest-memory handle to the backend** would let the backend read resources itself, but it puts
  the guest address space behind the API seam. Instead the **frontend** reads guest memory (it has
  `GuestMemory`), content-hashes and extracts bytes, and hands the backend plain data - which is what
  keeps `orbistoun-gpu` free of any graphics API (D029, principle 12). Content-addressing also makes
  invalidation automatic: a guest that rewrites a resource changes its bytes, so its id changes and
  it is a new resource, with no dirty-tracking needed for correctness.

## What this commits to, and what it leaves open

Committed: the resource seam is residency-shaped and content-keyed; host objects are the backend's to
own and evict; guest memory is the frontend's to read. Left open (and deliberately not pre-built):
the fixed-function state and pipeline assembly (a `VkPipeline` is the backend's internal concern,
keyed by bound shaders + a state hash, never named by the frontend - principle 12), eviction policy
(`release` defaults to keep; LRU and re-provide-on-`UnknownResource` come with memory pressure), and
the thin driver that runs `ensure_resident` → `execute` → `present`, which lands with the first
executing command.

## The first slice built under this

`VulkanBackend::ensure_resident`'s `Shader` arm creates a real `vk::ShaderModule` from the SPIR-V,
cached by id, destroyed on drop. Because the driver accepts it, this validates that the translator's
output is a module a Vulkan implementation takes - a stronger signal than "the words begin with the
SPIR-V magic". `RecordingBackend` records residency so the seam is testable with no device. Draws
still refuse honestly (D010) until execution lands.
