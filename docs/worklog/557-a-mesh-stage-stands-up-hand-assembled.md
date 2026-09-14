# 557. A mesh stage stands up, hand-assembled, before anything is translated into it

**2026-09-14** - orbistoun-spirv and orbistoun-gpu-vulkan, after worklog 556, for D688

D688 decided what a guest's NGG primitive shader becomes on the host: a mesh shader, because
the correspondence is one-to-one rather than an analogy. A mesh shader declares how many
vertices and primitives its workgroup will emit, writes the primitive's indices, and writes
per-vertex outputs - `MSG_GS_ALLOC_REQ`, `exp prim`, then `exp pos` and `exp param`, in that
order, in one workgroup.

None of that had been run. It has now: a hand-assembled mesh module draws a triangle on the
device, and the frame is **byte-identical** to the same triangle drawn through the vertex
stage.

Nothing is translated yet. That is deliberate and is the order D549 argues for - build the
oracle before the thing it will judge, because everything else is verified against material
this project generated and cannot be wrong in a way its own tests would notice. The fragment
work went the same way: an oracle vertex module first, then a translated fragment shader
checked against it.

## 1. The numbers were measured, not recalled

A mesh module needs a capability, an execution model, three execution modes, a built-in for
the index array and an instruction that declares the counts. Every one is a number, and none
of them is in the core SPIR-V this project already emits.

So they were read out of a compiled reference: a GLSL mesh shader through the SDK's compiler,
disassembled with `spirv-dis`, and the values taken from its words.

| | |
|---|---|
| capability `MeshShadingEXT` | 5283 |
| execution model `MeshEXT` | 5365 |
| mode `OutputVertices` | 26 |
| mode `OutputPrimitivesEXT` | 5270 |
| mode `OutputTrianglesEXT` | 5298 |
| built-in `PrimitiveTriangleIndicesEXT` | 5296 |
| `OpSetMeshOutputsEXT` | opcode 5295 |

The structure came from the same place, and one piece of it is not a choice: the per-vertex
outputs are an **array of a `Block` struct** whose first member carries `Position`. A bare
array of `vec4` decorated `Position` is what a vertex shader has, and it is not what this stage
takes.

This is the same discipline the instruction tables use - a reference decides the encoding, and
what is written here is what it said.

## 2. Two things the validator told me

`spirv-val` refused the first module for one reason: `SPV_EXT_mesh_shader` requires SPIR-V 1.4
and the builder emits 1.3. The comment on the version constant says 1.4 is deliberately avoided
because from there an entry point must list *every* global variable in its interface, which is
easy to get wrong. A mesh module's globals are exactly its three output arrays, so listing them
is no burden - and `VERSION_1_4` now exists with that reasoning attached, declared only by
modules that need it.

With that, the module validates, and the draw produces no validation-layer errors either.

## 3. The device, and the honest report

`VK_EXT_mesh_shader` is enabled where the device offers it, with its feature, the same way the
three features in D689 are. `Properties::mesh_shading` says whether the stage was **enabled**,
not whether the hardware has it - the rule that report already follows - and the test skips
with a sentence naming the device when it cannot: *a device without it can run a frame's pixel
shaders and not its geometry*. Both GPUs in this machine offer it.

The draw path now knows which stage feeds the fragment stage. A mesh pipeline binds a mesh
stage and dispatches **one workgroup** rather than three vertices, because a mesh shader
decides for itself how many vertices come out of it - which is exactly what makes it the right
host stage for a wave that announces its own output.

## 4. What the test asserts

Every pixel is the colour the mesh shader gave its vertices, over a clear it never writes; and
the frame is compared byte for byte with the vertex path's frame over the same triangle. The
second assertion is the one that means something, because the two paths share a fragment
shader and an attachment and differ only in what produced the geometry.

What it cannot assert is that a *translated* primitive shader would do this. There is nothing
translated in it. That is the next unit, and it now has something to be checked against.

## 5. Files

- `crates/orbistoun-spirv/src/lib.rs` - the measured constants, `VERSION_1_4`, and
  `triangle_mesh_module`; `examples/emit-minimal.rs` writes it out for the validator.
- `crates/orbistoun-gpu-vulkan/src/compute.rs` - the extension, the feature, the report row.
- `crates/orbistoun-gpu-vulkan/src/framebuffer.rs` - `draw_mesh_with`, the geometry a pipeline
  was built for, and the dispatch that follows from it.
- `crates/orbistoun-gpu-vulkan/tests/mesh.rs`.

## Next

1. Translate the GL cube's vertex program into this stage: `s_sendmsg` becomes the output
   declaration, `exp prim` becomes the index write, `exp pos` and `exp param` become per-vertex
   outputs. The one unknown is the allocation request's payload split, which is `assumed` in
   D688 and filed as `REQ-20260914T1720Z-9c4a`; for this shader its own value settles it either
   way.
2. With that, the console's own geometry and its own pixel shader would run in one frame, and
   the oracle record's hash becomes a question worth asking.
3. The image subsystem, still, for record B.
