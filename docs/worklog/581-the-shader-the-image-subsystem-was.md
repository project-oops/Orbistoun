# 581. The shader the image subsystem was built for draws, and it reads two attributes

**2026-09-15** - orbistoun-spirv and orbistoun-gpu-vulkan, after worklog 580

Oracle record B's textured pixel shader is the reason the image subsystem exists. It has
translated since worklog 568 and had **never been run**.

It runs now, and every pixel of the frame is a texel of the bound texture, with the four
quadrants holding the four texels in the places the coordinate names - the same four-way claim
the hand-written oracle and the hand-assembled translation make, over a shader a console ran.

## 1. The first attempt drew black, which was informative

Not magenta - the clear - so the shader ran and exported. Black with full alpha, everywhere the
assertion looked.

Two draws separated it. With the one supplied attribute swept from zero upward, the frame is dark
where the attribute is near zero; with the same attribute set to ones, **every pixel is the first
texel**. So the shader modulates the sampled texel by that attribute, and its texture coordinate
comes from somewhere the test was not supplying - constant, therefore one texel everywhere.

A scan of the module's own decorations said where: it declares inputs at locations **zero and
one**. The oracle's vertex module writes one varying.

## 2. The oracle grew a second varying, and that is the real change

`interpolated_vertex_module` carries one varying at location zero. A shader reading two, fed by
it, gets an unwritten input for the second - which is the same value at every pixel, so the frame
says nothing about whether the coordinate reached the sample. **It would have passed a weaker
assertion**, and that is the failure mode worth naming: an input that cannot distinguish the
right answer from the wrong one, which is the third time this session has met one.

The builder now takes any number of varyings, one per location in order, and the single-varying
entry point is a one-element call into it. Every existing caller is unchanged.

## 3. What the frame is and is not

It **is** a shader a console ran, translated here, sampling a texture at a coordinate it
interpolated, with the result modulated by a second interpolated attribute, and every pixel
landing on the texel its coordinate names.

It is **not** the console's frame. The record captured the command stream and the shader payload,
not the texture, so the texels are this test's; and the vertex program is the oracle's triangle,
so the coordinate sweep is this test's arrangement too. Which texture a translated sample reads
is the one the pipeline bound, for the reason D690 gives.

## 4. Every image instruction has now drawn

The two sampling forms, the levelled sample, the texel fetch, the store - and now the console's
own textured shader, which uses the first of them. The subsystem is complete in the sense that
matters: nothing in it is a claim about a module that no frame has checked.

## 5. Files

- `crates/orbistoun-spirv/src/lib.rs` - `interpolating_vertex_module`, with
  `interpolated_vertex_module` as its one-varying case.
- `crates/orbistoun-gpu-vulkan/tests/console_textured.rs` - the frame.

## Next

1. `REQ-20260914T2348Z-4e71` - which register carries a buffer address, for a window the
   pipeline derives rather than is told.
2. `REQ-20260914T1720Z-9c4a` - the payload split D688 assumes.
3. The record's pixel hash, which is the claim neither of these reaches: it needs the vertex
   buffer and the texture the records did not capture.
