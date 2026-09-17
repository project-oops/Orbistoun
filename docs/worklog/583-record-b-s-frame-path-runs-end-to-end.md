# 583. Record B's frame path runs end to end, with nothing hand-written in it

**2026-09-15** - orbistoun-gpu-vulkan, after worklog 582

Record A's two shaders have drawn together since worklog 565, and record B's - the textured pair -
never had. They do now.

The console's **primitive shader** fetches vertices out of guest memory, exports their positions
and their texture coordinates. The console's **textured pixel shader** samples with what the
hardware interpolated from those. Every quadrant of the frame holds the texel its coordinate
names, and every pixel is a texel rather than the clear.

Neither shader was written here. Both came out of one captured frame.

## 1. What had to exist first

Everything from the last eighteen worklogs, which is the point of writing it down: the mesh stage
(D688), the window with a base and a span covering both the vertex buffer and the canary
(worklogs 561, 565, 570), the image subsystem and its binding (566), the sampling translation and
D690, and an oracle vertex module able to carry more than one varying (581).

The only new code is one entry point: a mesh draw that binds a texture. Every other mesh path
binds the default one white texel, which is enough for a pipeline to be valid and not enough to
see.

## 2. The vertex layout was already known

Twelve words a vertex: four of position, four of colour, four of texture coordinate. Record A's
frame established that by drawing from it and needed only the first two groups. This test needed
the third, and it was where that layout said it would be - which is the first time that part of
the layout has been used for anything.

## 3. What it is not

**Not the console's frame.** The record captured the command stream and the shader payload, and
neither the vertex buffer nor the texture - so the vertices and the texels are this test's. Which
texture a translated sample reads is the one the pipeline bound, for the reason D690 gives.

The record's pixel hash remains a different claim, and it needs exactly the two things the record
does not contain.

## 4. Two tests, and the first is not redundant

The pixel shader is also driven from the oracle's own triangle, which **isolates** it: what it
draws then depends on nothing else that could be wrong. The pair test is the stronger claim and
the weaker diagnostic - if it failed, either shader could be the reason. Keeping both is what
makes a failure say which.

## 5. Files

- `crates/orbistoun-gpu-vulkan/src/framebuffer.rs` - `draw_mesh_over_texture`.
- `crates/orbistoun-gpu-vulkan/tests/console_textured.rs` - the pair, drawing.

## Next

1. `REQ-20260914T2348Z-4e71` - which register carries a buffer address, for a window the pipeline
   derives rather than is told.
2. `REQ-20260914T1720Z-9c4a` - the payload split D688 assumes.
3. The pixel hash, which needs a capture carrying the vertex buffer and the texture.
