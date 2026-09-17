# 639. A draw issues the guest's decoded vertex count, not a fixed three

**2026-09-16** - the graphics `Draw` stopped hardcoding `cmd_draw(3, 1, 0, 0)`: it now issues the
vertex count, instance count and first vertex the draw packet already carried, the first slice of
vertex-input

## What this joins

The draw count was decoded long ago - `DrawKind::Auto { vertices }` reads the `DRAW_INDEX_AUTO`
packet's first body word (worklog 585), corroborated because three is exactly what the GL cube's
primitive shader declares it emits - and it reached the backend as `RenderCommand::Draw { vertices,
instances, first_vertex }`. But the executor threw it away: `framebuffer`'s draw hardcoded three
vertices, one instance, because that is what the fullscreen-triangle test shader supplies. This
threads the decoded values through.

- `Geometry::Vertex` carries a `VertexDraw { vertices, instances, first_vertex }` rather than being
  parameterless; `record` issues `cmd_draw` with them (`first_instance` stays zero - nothing decodes
  it, and zero is what it means, not an invented value).
- The backend's `draw_graphics` takes the draw's parameters and passes them; `execute`'s `Draw` arm
  reads them straight off the command.
- The harness entries that draw a fullscreen triangle keep doing so through a named `VertexDraw::TRIANGLE`
  constant, so the change is visible where it is deliberate rather than scattered as a literal three.

## Made to fail

`a_draw_issues_its_decoded_vertex_count` (device-gated): with one bound fullscreen-triangle pipeline,
a draw of **zero** vertices issues no geometry and the frame stays the interim clear (opaque black),
while a draw of **three** paints it the fragment's green. A backend that ignored the count and always
drew three would paint both, so the black frame is the half that proves the count reached `cmd_draw` -
counting a pass (green) alone never could.

## What is interim, named as such

- **The geometry source is not bound yet.** The count says how many times the vertex shader runs, but
  a guest's vertex shader fetches each vertex from the guest-memory window (the translator's flat
  `GUEST_MEMORY` buffer, worklog 635), and the graphics path still binds the default empty window -
  so a guest's real geometry reads zeros until that window is seeded from a resident buffer. That is
  the next piece: the graphics half of the buffer arm, the counterpart of compute's `dispatch_into`,
  which needs `framebuffer::render` to bind an external resident buffer at binding 1 rather than the
  one it allocates. Until then the count is honoured over the interim fullscreen triangle, which is
  enough to prove the count drives the draw and not enough to draw a guest's mesh.
- **`first_vertex` is always zero and instances always the decoded count** - both are carried
  honestly, but the frontend only ever emits `first_vertex: 0` today, so the non-zero path is
  reachable through the backend and not yet through a real stream.
- Indexed draws, viewport and clears still refuse by name (D010).

## Gate state

`cargo test --workspace` **2401 passed, 0 failed** (+1 device-gated); `cargo clippy --workspace
--all-targets -D warnings` clean; fmt clean; identity scan exit 0. No commit.
