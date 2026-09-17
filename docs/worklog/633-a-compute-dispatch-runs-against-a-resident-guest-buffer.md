# 633. A compute dispatch runs against a resident guest buffer, and the throwaway width is gone for it

**2026-09-16** - the `Buffer` arm of the resource seam, and the refactor that lets a dispatch bind a
resident buffer instead of a scratch one: a compute shader now writes the guest's own buffer

## What this adds

Worklog 632 ran a compute dispatch through the executor, but into a fixed-width throwaway buffer
(`DISPATCH_WORDS`) because there was no way to give the dispatch a *guest's* buffer. This closes that
for compute: the `Resource` enum grows a `Buffer` arm, the backend makes a guest buffer resident, and
a dispatch binds the resident buffer and writes into it - read back, it holds what the shader wrote.

## The enabling refactor, behaviour-preserving

`compute::dispatch` was fully self-contained - it created *and destroyed* its two storage buffers,
pipeline and module every call. Extracted `dispatch_core(device, queue, family, module, binding0,
binding1, groups)`: it builds the pipeline, records, submits, reads both buffers back, and releases
everything **except the buffers** - the caller owns those. `dispatch` now creates two throwaways,
calls the core, and destroys them; its four tests pass unchanged, which is what makes the extraction
safe. `dispatch_into(output, module, groups)` is the new variant: it binds a caller-provided
(resident) buffer at binding 0, backs binding 1 with a scratch of the same width, and leaves the
output buffer alone for the caller to reuse.

## The `Buffer` arm (D701)

`Resource::Buffer(&[u8])` joins `Shader`; the backend's `ensure_resident` match is exhaustive, so
adding it forced the arm rather than letting a new kind slip through silently. `VulkanBackend` now
holds `buffers: BTreeMap<ResourceId, DispatchBuffer>`: on `ensure_resident(Buffer)` it creates a
host-visible `vk::Buffer`, uploads the guest's bytes into it, and caches it by content id; a
`BindBuffer` records it as the dispatch's output; `Dispatch` runs `dispatch_into` against it when one
is bound, and the interim fixed width only serves the no-buffer path now. Buffers are destroyed with
the backend, beside the shader modules.

Guest memory is still read on the frontend side and handed to the backend as plain bytes - the arm
takes `&[u8]`, not a guest pointer - so `orbistoun-gpu` still names no graphics API (D701, principle
12).

## What is still interim

- `BindBuffer`'s `offset` and `length` are not read yet: the whole buffer binds, at binding 0, which
  the fixed two-storage-buffer convention every translated module declares allows. A guest binding
  several buffers at named slots is the next refinement.
- Nothing on the **frontend** produces `Buffer` resources yet - the V#/SRT decode (D204) that reads
  a guest's buffer descriptors from its scalar registers is the other half, and it feeds this arm
  when it lands. Until then the arm is exercised by a constructed submission, which is a real test of
  the backend capability, not of a guest's frame.

## Made to fail

- `a_dispatch_writes_into_a_bound_resident_buffer` (gpu-vulkan, device-gated) - a buffer is made
  resident, bound, and a `storage_buffer_write_module` writes its constant into it; reading the
  buffer back gives the constant. Fails against a dispatch that ignores the bound buffer (writes the
  throwaway) or that never runs.
- The four `dispatch` tests still pass, which is the guard on the refactor being behaviour-preserving.

## A flake worth noting

One full-workspace run reported a single transient Vulkan failure that did not reproduce across two
further full runs (2,387 pass) or a Vulkan-only run (45 pass). Dispatches are serialised by the
session mutex, so it is a device-level transient rather than a race the change introduced - recorded
so a future intermittent is not read as new.

## Gate state

`cargo test --workspace` 2,387 pass / 0 fail (confirmed across two runs); `cargo clippy --workspace
--all-targets -D warnings` clean; fmt clean.
