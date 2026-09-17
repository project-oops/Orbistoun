# 636. The first graphics frame through the executor: a bound vertex+fragment pipeline draws

**2026-09-16** - the render backend's `Draw` no longer refuses: it runs a resident vertex and
fragment shader through a graphics pipeline and reads the frame back, the graphics counterpart of the
compute path

## What this adds

The executor could run a compute dispatch (worklog 632-633) but refused every graphics command. This
is the first graphics execution, on the path that actually renders a title's frame. A `Draw` now:

- takes the vertex and fragment shaders bound to the pipeline (`BindShader` learned the `Vertex` and
  `Fragment` stages, not just `Compute`),
- runs them through the tested graphics pipeline in `framebuffer.rs`, clearing a colour attachment
  and drawing three vertices,
- and reads the frame back, kept as `last_frame()` for a caller to inspect.

`framebuffer::draw_triangle` is the entry the backend calls - the graphics counterpart of
`compute::dispatch`: give it the two translated modules and the attachment size, get the pixels they
drew. It reuses the whole tested render path (attachment, render pass, pipeline, draw, readback).

## Verified on the device

`a_bound_vertex_and_fragment_pipeline_draws_a_frame`: a `fullscreen_triangle_vertex_module` and a
`constant_colour_fragment_module([0,1,0,1])` are made resident, bound to their stages, and a `Draw`
renders them. The centre pixel reads back `[0, 255, 0, 255]` - the fragment's green, in the
`R8G8B8A8_UNORM` order the attachment names. A translated pipeline drew a real frame through
`RenderBackend::execute`. `a_draw_with_no_shaders_bound_is_refused` pins the other half: a draw with a
stage unbound is refused, not run as half a pipeline.

## What is interim, named as such

- **The render target is a fixed 64×64 square** (`RENDER_WIDTH`) cleared to black. A guest's target
  has its own dimensions and clear, decoded from the CB registers a stream sets; that decode is the
  next graphics piece, and the fixed square is enough to run a translated pipeline end to end now.
- **`Draw` ignores its vertex count and instances** and draws the three the fullscreen-triangle
  vertex shader supplies; a guest's vertex buffers and counts come with the vertex-input decode.
- Indexed draws, render targets, viewport and clears still refuse by name (D010) until their
  execution lands.

## A test the change dated

`every_command_is_refused_by_name` executed a `Draw` and asserted it was refused - true when nothing
drew, false now that `Draw` runs. Repointed at `SetRenderTargets`, a command whose execution genuinely
has not landed, so the guard still checks what it means to: an unimplemented command names itself
rather than silently drawing nothing.

## Gate state

`cargo test -p orbistoun-gpu-vulkan` 10 lib pass (2 new device-gated, one a real graphics render);
`cargo clippy --workspace --all-targets -D warnings` clean; fmt clean. Module and struct docs updated
- the backend no longer "refuses every command". Full-workspace gates running.
