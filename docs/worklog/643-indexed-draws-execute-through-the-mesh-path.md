# 643. Indexed draws execute through the mesh path, refused only on a vertex pipeline

**2026-09-16** - a `DrawIndexed` is no longer refused by name: a guest's indexed geometry is a mesh
shader that reads its own indices from the window, so its indexed draw *is* its mesh draw, and it
renders; a vertex pipeline's indexed draw is refused for want of a host index buffer, honestly

## What an indexed draw is, on this hardware

The frontend has decoded `DRAW_INDEX_2` into a `DrawIndexed` command since worklog 628, and the
executor refused it. Wiring it up meant first asking what an indexed draw *is* here, and it is not a
host indexed draw. A guest's geometry program is an NGG primitive shader that translates to a **mesh**
shader (D688), and a mesh shader fetches its own vertices and **its own indices** from guest memory -
there is no host index buffer to bind, because the geometry engine's index fetch is inside the shader.
The index buffer is a region of the guest-memory window the executor already binds (worklog 641).

So a guest's `DrawIndexed` takes the **mesh path** - the same `cmd_draw_mesh_tasks` a `Draw` of mesh
geometry takes - and the "indexed" part is the mesh shader reading indices from the window, not a
`cmd_draw_indexed` with a bound buffer. The routing:

- **Mesh geometry**: `DrawIndexed` draws through the mesh path, exactly as `Draw` does. It executes.
- **Vertex pipeline**: an indexed draw *would* need a host index buffer, which the executor does not
  bind. Refused by name (D010) - reachable only by a vertex module, which is a test shader, since a
  guest's geometry is never a host vertex shader. Drawing it without the index buffer would draw the
  wrong geometry while looking like it worked, which is the plausible-output failure the refusal
  exists to prevent.

## Made to fail

- `an_indexed_draw_routes_to_the_graphics_path` (no device): a `DrawIndexed` with nothing bound gets
  the *graphics* refusal ("no vertex and fragment shaders bound"), not the generic "DrawIndexed" it
  got before - which is what proves it reached `draw_graphics` rather than the by-name refusal.
- `an_indexed_draw_of_mesh_geometry_executes` (device + mesh): a `triangle_mesh_module` drawn through
  a `DrawIndexed` comes back the fragment's green - proof-by-structure, since a mesh module on the
  vertex path fails pipeline creation, so a green frame means it routed to the mesh path.
- `an_indexed_draw_on_a_vertex_pipeline_is_refused` (device): a bound vertex module's `DrawIndexed` is
  refused by the exact name, not drawn as a non-indexed draw.

## What is interim, named as such

- **The index count does not change the mesh draw.** A mesh draw is one workgroup, and a translated
  mesh shader emits the fixed count its `MSG_GS_ALLOC_REQ` declared (worklog 585), so `indices` rides
  in the command but does not drive `cmd_draw_mesh_tasks` - correct for the one-triangle draws in
  hand, and a larger indexed draw needing several workgroups is a later refinement.
- **Not verified that a mesh shader reads real indices**, for the same reason the window read is
  unverified against a guest shader (worklog 641): no hand-assembled fixture reads an index buffer,
  only a real translated guest shader does. This lands the routing and the honest refusal; a captured
  indexed frame is a later unit.
- The host vertex-pipeline indexed draw (`cmd_draw_indexed` with a bound index buffer) is refused, not
  built - a guest does not use it, so it would pay off only hypothetically (principle 12).

## Gate state

`cargo test --workspace` **2410 passed, 0 failed** (+3: one no-device, one device+mesh, one device);
`cargo clippy --workspace --all-targets -D warnings` clean; fmt clean; identity scan exit 0. Module
status doc updated - indexed draws are no longer in the refused-by-name list. No commit.
