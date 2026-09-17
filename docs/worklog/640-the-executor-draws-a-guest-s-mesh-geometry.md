# 640. The executor draws a guest's mesh geometry, routed by the module not the command

**2026-09-16** - a `Draw` whose bound geometry shader is a mesh module runs through the mesh path
(`cmd_draw_mesh_tasks`), not the vertex path - the first half of the geometry-source arc, because a
guest's geometry stage is a mesh shader (D688) and the executor drew only vertex shaders before

## The fork this resolves

Worklog 639 taught the graphics `Draw` to issue the guest's vertex count through `cmd_draw`. But that
is the *vertex* path, and a guest's geometry never takes it: an NGG primitive shader translates to a
**mesh** shader (D688), drawn by `cmd_draw_mesh_tasks`, which declares its own vertex and primitive
counts rather than taking a count from the draw. So the executor could run a fullscreen-triangle test
shader and not a single guest's geometry.

The question was where the mesh-versus-vertex decision belongs. The guest binds a `Vertex` stage
either way - the host mesh-ness is a *translation* detail, not something the guest expressed - so
putting it in the command (a `RenderCommand` variant, or a stage flag) would carry a host concept
across the frontend seam that principle 12 keeps clean. It belongs to the **module**: a translated
mesh shader's SPIR-V execution model is `MeshEXT`, a vertex shader's is `Vertex`, and the backend owns
the module. So the backend reads the execution model and routes on it.

## What was built

- `is_mesh_module(&[u32])` reads the execution model of a module's first `OpEntryPoint` (operand one),
  walking the instruction stream past the five-word header. The three SPIR-V constants it needs
  (magic, the `OpEntryPoint` opcode, the `MeshEXT` model) are transcribed from the Khronos SPIR-V
  specification - an open standard, documented constants rather than a hypothesis, unlike a register
  offset. A backend cannot depend on `orbistoun-spirv` (that is the test module *builder*, and a
  backend needing it would be the layering wrong), so it names the three values itself.
- `draw_graphics` routes: a mesh module goes to `framebuffer::draw_mesh_with` (one workgroup, which is
  what a guest's primitive shader is), a vertex module to `draw_vertices` with the guest's decoded
  count. A mesh module on a device **without** a mesh stage is refused by name (D010), not issued as
  an invalid command that would fault the process.

## Made to fail

- `the_geometry_stage_is_routed_by_the_module_not_the_command` (no device): the reader returns true
  for `triangle_mesh_module`, false for `fullscreen_triangle_vertex_module`, false for a fragment
  module, and false (no panic) for non-SPIR-V and empty input. This pins the routing decision
  everywhere, including machines with no mesh stage.
- `a_bound_mesh_geometry_shader_draws_through_the_mesh_path` (device + mesh stage): a
  `triangle_mesh_module` bound as the geometry stage and drawn comes back the fragment's green. The
  made-to-fail is structural rather than asserted - a mesh module drawn through the *vertex* path
  fails pipeline creation, because its execution model is not vertex, so a green frame is itself the
  proof the routing worked.

## What is still interim - the second half of the arc

**The geometry source is not fed yet.** The mesh shader runs, but a guest's primitive shader fetches
each vertex from the guest-memory window (the translator's flat `GUEST_MEMORY` buffer, worklog 635),
and the graphics path still binds the default *empty* window - so a `triangle_mesh_module` with its
corners baked in draws, and a guest's shader reading the window would read zeros. Binding a resident
buffer as that window is the next unit: the graphics counterpart of compute's `dispatch_into`, which
needs `framebuffer::render`/`build_pipeline` to bind an external resident buffer at binding 1 rather
than the one it allocates. With that, the actual captured frame (`submitted_frame.rs`, today drawn
through the harness) runs through the executor's `Draw`.

Indexed draws, viewport and clears still refuse by name (D010).

## Gate state

`cargo test --workspace` **2403 passed, 0 failed** (+2: one pure, one device+mesh-gated); `cargo
clippy --workspace --all-targets -D warnings` clean; fmt clean; identity scan exit 0. No commit.
