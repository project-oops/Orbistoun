# D688 - An NGG primitive shader translates to a mesh shader, not a vertex shader

**assumed** - 2026-09-14

The GL cube's vertex program stops at its sixth instruction, `s_sendmsg`, and both that and
`exp prim` have been sitting in `BLOCKED` waiting for an answer to one question: what is a
primitive shader, to this translator? This is that answer, made without input and recorded so
it stops being re-asked. Nothing is implemented against it yet.

## What the guest's shader actually is

The console ran this, and the frame it produced is oracle record A
(oops-sdk `docs/hardware/agc-gl-cube-oracle-fw1240.md`; orbistoun worklog 545). One wave does
all of it:

1. Saves the entry execution mask, writes `m0`, and sends `MSG_GS_ALLOC_REQ` - which tells the
   geometry engine how much output the wave is about to produce.
2. Narrows the mask to **one** lane, builds a packed primitive in a register, and exports it
   with `exp prim`.
3. Widens the mask to **three** lanes, one per vertex, loads each vertex's attributes, and
   exports position and two parameters.

So a single hardware wave carries both the primitive and its vertices, decides for itself how
many of each there are, and announces that before exporting anything. That is not what a host
vertex shader is. A vertex shader is invoked once per vertex, cannot see the primitive, and
never declares a count - the draw call did that.

## The options

**A. A mesh shader.** The correspondence is one-to-one and it is not an analogy: a mesh
shader's first act is to declare how many vertices and primitives its workgroup will emit
(`OpSetMeshOutputsEXT`), then it writes primitive indices and per-vertex outputs. That is
`MSG_GS_ALLOC_REQ`, `exp prim`, and `exp pos`/`exp param`, in that order, with the same
meaning and the same one-workgroup-does-both shape. Needs `VK_EXT_mesh_shader` and the
`MeshShadingEXT` capability, and this project's device currently requests no features at all
(D552), so it is a real cost rather than a free one.

**B. A vertex shader, dropping the primitive half.** Translate the per-vertex path and ignore
the allocation request and the primitive export, letting the host's input assembler build
triangles from the draw. For *this* shader it would produce a correct frame, because its
primitive export is the identity - vertices 0, 1, 2, one primitive, nothing culled.

**C. Compute, writing vertices to a buffer that a second draw consumes.** Portable, needs no
extension, and costs a round trip plus a whole buffer-management subsystem.

## The decision, and why not B

**A.** B is rejected even though it is cheaper, and would work today, and the only NGG shader
this project has is one it would handle.

The reason is what NGG is *for*. A primitive shader exists so the shader itself can discard
primitives before they cost anything - back-face and small-triangle culling, index compaction -
which means a compiled NGG shader from a real title routinely exports fewer primitives than it
was given, with the count computed at runtime. Under B that shader translates without
complaint and draws primitives the guest culled. Silent, frame-dependent, and precisely the
failure this project refuses one level down in every stub.

Recognising the identity case and refusing everything else is the version of B worth
considering, and it is still a shortcut that constrains: the discriminator is a pattern match
on an instruction sequence, which is the kind of thing that works for the shader it was
written against and becomes a liability the moment a second one differs by a register. D028
says a shortcut that constrains the design is never worth the time it saves, and this project
has no deadline.

C stays recorded as the fallback if the extension turns out to be unavailable on the host this
runs on. It is a fallback, not a plan.

## What is not yet known, and how to find out

The allocation request's payload. Our shader writes `m0 = 0x1003` and the comment beside it
says one primitive of three vertices, which is consistent with the vertex count in the low
half and the primitive count above it - and one sample cannot separate that from several other
field splits. It is `assumed`, and the way to settle it is cheap: emit a shader that asks for a
different pair, say two primitives of four vertices, and see which bits move. That is a probe
for the same toolchain path worklogs 548 and 549 used, or an obSCEne request.

## What this costs when it is built

A `Stage::Mesh` in the translator; the `MeshShadingEXT` capability and the mesh execution
model in the emitter, which has neither; `OpSetMeshOutputsEXT` from the allocation request;
`exp prim` writing primitive indices; `exp pos`/`exp param` writing per-vertex outputs rather
than being refused; and a pipeline in `orbistoun-gpu-vulkan` that binds a mesh stage and a
device that asks for the feature. Each is ordinary work. None of it is done, and until it is,
`s_sendmsg` stays in `BLOCKED` with its reason pointing here.

## Consequences

- The blocked entry for `s_sendmsg` is a deferral with a destination rather than an open
  question.
- `exp prim` will need the same treatment; it is not in `BLOCKED` yet only because no shader
  has reached it.
- The GL cube's vertex program is a *good* first mesh shader precisely because its primitive
  export is trivial: it exercises the plumbing without needing culling to be right.
