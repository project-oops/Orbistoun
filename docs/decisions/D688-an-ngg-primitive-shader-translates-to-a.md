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

## The allocation request's payload, now measured

Our shader writes `m0 = 0x1003` for one primitive of three vertices. Which bits are the vertex
count and which the primitive count was the one assumption in this translation, and it is now
**measured**: the low field is vertices, the high field is primitives.

`REQ-20260915T1652Z-6fad` (sweep `20260915-192617`) submitted the primitive-draw triangle - body
unchanged, one primitive of three vertices - with three `m0` literals and read the fence and the
colour target back:

| `m0` | low 12 = verts, high = prims | drew? |
|---|---|---|
| `0x1003` | 3 verts, 1 prim - exact | yes, red, fence `0xbeefcafe` |
| `0x1004` | 4 verts, 1 prim - surplus verts | yes |
| `0x3001` | 1 vert, 3 prims - starved of vertices | no, fence stayed `0x11111111`, shader never ran |

Starving the vertex count stops the draw and inflating it does not, so vertices are low and
primitives high. The translator already read it that way (`MESH_COUNT_BITS = 12` in
`model.rs`), so no code changed; the assumption became a measurement.

**Two earlier "resolutions" were not measurements and must not be cited.**
`REQ-20260914T1720Z-9c4a` and `REQ-20260915T1211Z-5b01` came back "delivered" from sweeps
`20260915-125124` and `-152001`, but `check_agc_ngg_gs_alloc_req` there only prints a static table
of the answer - no queue, no submit, no shader (worklog 602). **And 6fad's own stated conclusion,
`prims = m0 >> 16`, is wrong**: under a sixteen-bit split variant A (`0x1003`) is zero primitives
and would not draw, yet it drew a triangle. The rows are sound; the prose over them is not. The
exact boundary is not uniquely pinned by three samples (anything from bit 3 to bit 12 fits), but
twelve reads oops-sdk's own encoding correctly and real NGG counts never approach 2^12 (worklog
603).

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
