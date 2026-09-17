# 566. A texture reaches a device, and the level a guest asks for turns out to be a different instruction

**2026-09-14** - orbistoun-spirv and orbistoun-gpu-vulkan, after worklog 565

Oracle record B is the GL cube with a texture on it, and its pixel shader ends in
`image_sample_lz` - the last family the translator refuses. Worklog 565 said the image subsystem
was next. **The host half of it now exists**: an image, a view, a sampler, a descriptor and two
hand-assembled modules that read a texel through them, drawing on a GPU with nothing for the
Khronos validation layer to say.

Nothing is translated. That is the order D549 argues for and the order the fragment work and the
mesh stage both followed - build the thing a translation will be measured against first, so that
when the translation arrives there is something to disagree with it.

## 1. The numbers, measured

Four encodings this crate had never emitted, read out of compiled output rather than transcribed:
a GLSL fragment shader sampling a `sampler2D` was compiled with the SDK's compiler and
disassembled.

- `OpTypeImage`, and its seven operands - element type, then six literals saying that this is a
  two-dimensional, non-depth, non-array, single-sampled texture used with a sampler and of no
  declared format.
- `OpTypeSampledImage`, `OpVectorShuffle`, and the storage class a descriptor-bound image lives
  in, which is not the one a buffer lives in.
- `OpImageSampleImplicitLod`.

Then a second reference, and this is the part worth having written down.

## 2. A guest's `_lz` is not the instruction the obvious oracle emits

The first oracle sampled with the implicit form, which is what a fragment shader gets by default
and what a first reference shader compiles to. A guest's instruction is `image_sample_lz` - the
`lz` is *level zero, named*. Those are two different SPIR-V instructions, not one instruction
with an option: the explicit form carries a literal operand mask after the coordinate and then
the level, two operands the implicit form does not have.

A second reference - the same shader written with `textureLod(tex, uv, 0.0)` - gave
`OpImageSampleExplicitLod` and the mask value that announces a level follows.

That matters because **an oracle emitting the other instruction is not an oracle for this
translation.** It would have drawn the same picture on this texture, which has one level, and the
difference would have surfaced later as a translation checked against something it does not
resemble. The module builder now takes which level form to emit, and the device test runs both
through the same assertions and then compares the two frames byte for byte - a texture with one
level has to look the same either way, and a difference would mean one of them is reading
something else.

## 3. The shape table refused the module, which is the design working

The module builder's own checker rejected the first sampling module outright: `UnknownOpcode`. An
opcode with no row in its shape table cannot be walked for identifiers, and it says so rather
than skipping the instruction and reporting a clean module it did not check. Four rows added, and
each one is a claim about which operands are identifiers and which are literals:

- The six literals after an image type's element type are not identifiers. Reading the
  dimensionality as one would reject a module that is fine.
- A shuffle's component indices are positions in a vector, not identifiers.
- The explicit sampling form's operand mask is a literal at index four, and the identifiers it
  announces start at five - so the mask is named by neither list and the level that follows it
  is checked.

The implicit form's optional tail is *not* covered, and the row says so: emitting one means
extending the row. That is an honest gap rather than a silent one.

## 4. The texture, and two choices in it

**Optimally tiled, filled by a staging copy** rather than a linear image written through a
mapping. A linear image is fewer moving parts and was the first thing written; it is also not
*required* to support sampling on every implementation, where an optimally tiled
`R8G8B8A8_UNORM` is. A harness that can fail on a conforming driver is worth more code to avoid.
It also leaves the row stride to the driver rather than to this file - a linear image's rows are
not necessarily packed, and writing one as though they were puts every row but the first in the
wrong place.

**Created for every pipeline**, whether or not the fragment module samples. One white texel when
nobody asked for a texture. A binding nothing uses costs one descriptor; a binding a module uses
and the layout lacks is an invalid pipeline, and which of the two a translated module is is not
something this harness reads. The same judgement the two storage buffers already record, for the
same reason - worklog 554, where exactly that mismatch drew a correct-looking picture and
produced two validation errors.

The stage flags are the fragment stage alone. A guest's *pixel* shader is what reads a texture;
declaring a stage nothing here uses would assert something nobody has measured.

## 5. What the test asserts, and what it cannot

Four quadrants of one frame, each carrying a different texel of a two-by-two texture, with the
coordinate arriving as an interpolated varying because that is what a guest's textured shader
has. Four claims in one draw:

- the image is bound and read, because a pixel is a texel rather than the clear;
- the coordinate reaches the sample, because four coordinates give four different answers;
- the rows are the right way up, because the texel a larger `v` names is the one stored later -
  the one thing a staging copy can silently get backwards;
- nothing is filtered, because an exact byte match at a texel centre is only available from a
  sampler that does not blend neighbours.

Plus every pixel being *some* texel, which is the coverage claim: a frame that sampled correctly
in four places and left a band of clear between them would pass the four checks above.

It cannot assert that a translated `image_sample_lz` would do any of this. Nothing is translated.

One further thing is asserted where it costs nothing: the binding number. It lives in two crates
that cannot import each other - the module builder says where a sampled image *is* and the
backend says where one is *bound*, and a backend depending on the module builder would have the
layering backwards. A test with no device requirement compares them, so a mismatch fails
everywhere rather than only on a machine that has a GPU.

## 6. Gates, and two that were already red

444 tests across the six crates. Every device test runs clean under the Khronos validation layer
via `tools/validate-device.sh`, sampling included. Both emitted modules pass `spirv-val`.
Formatting, clippy across the changed crates, the provenance guard and the identity scan are all
clean.

The tree check reported three failures, and running them one at a time is what separated them:

- **The documentation gate was red, half of it from here.** A link in `wavefront.rs` written
  during the window work names its target twice, which rustdoc calls redundant. Fixed. The other
  half is older - a public doc comment in `agc.rs` linking to a private module - and is fixed
  too, because it is one line and a gate that is red for any reason cannot report a new fault.
- **The prose gate is red for about thirty files, none of them from here.** Every one offends at
  `HEAD` and is unmodified. The new test used a line-continued literal and no longer does; the
  rest is a backlog, raised separately rather than silenced by widening the ceiling, which the
  ceiling file's own contract forbids.
- **The generated-numbers gate cannot run.** This repository's `README.md` lost the markers that
  delimit its generated block, at `HEAD`, so the tool bails rather than checking - which means
  the counts in it have been drifting unwatched. Also raised separately.

Neither of the last two is from this work, and neither is left unrecorded.

## 7. Files

- `crates/orbistoun-spirv/src/lib.rs` - the image and sampling opcodes, the operand mask, the
  `UniformConstant` storage class, `Lod`, `sampling_fragment_module`, `TEXTURE_BINDING`, and four
  shape-table rows.
- `crates/orbistoun-spirv/examples/emit-minimal.rs` - both forms, for `spirv-val`.
- `crates/orbistoun-gpu-vulkan/src/framebuffer.rs` - `Texture`, `create_texture`,
  `upload_texture`, the third binding, `Bound`, and `draw_with_texture`.
- `crates/orbistoun-gpu-vulkan/tests/sampling.rs` - the frame, asserted.

## Next

1. Translate `image_sample_lz` onto it. **D690 settles how**: every sample reads the one bound
   texture, a second distinct descriptor is refused, and a write into either recorded register
   range invalidates the recording so the next sample is refused too. That third rule is what
   makes the first two an argument rather than a hope - without it a shader reloading eight
   registers would read two textures and pass.
2. A window per buffer, or a window the pipeline derives from the submission: one span covers a
   vertex buffer or a canary, not both.
3. `REQ-20260914T1720Z-9c4a`, which settles the payload split D688 assumes.
