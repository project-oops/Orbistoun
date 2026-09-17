# 568. The console's textured pixel shader translates, and the blocked list is empty again

**2026-09-15** - orbistoun-translate, orbistoun-gpu-vulkan and orbistoun-gpu, after worklog 566

Oracle record B is the GL cube with a texture on it. Its pixel shader ends in
`image_sample_lz`, and that instruction has been the last refusal in either captured stream for
as long as the streams have existed. **It translates now**, and the frame it draws is byte-identical
to the hand-written oracle worklog 566 built for exactly this comparison.

Both records report `translated 2` - a vertex program and a pixel shader each, nothing refused.

## 1. The decision was to not solve the hard half

The instruction names its texture in eight consecutive scalar registers and its sampler in four
more. Those hold a descriptor: a base address, an extent, a format, a tiling mode - written by
the shader itself, usually loaded from a pointer the command stream handed it. What they
describe is a surface at a guest address that **no host image stands behind**.

Decoding them would mean constant-folding the scalar loads that filled them, reading the fields,
and then finding the answer points at guest memory this backend has never uploaded. That is a
descriptor subsystem: a decoder, a surface cache, a format table, an upload path. It is the
right eventual answer and it is not a prerequisite for translating one instruction.

So D690 maps every sample in a module onto the **one texture the pipeline bound**, and refuses a
module where that cannot be the whole story. Three rules:

1. The first sample records the register numbers its two descriptor operands name.
2. A later sample naming a different pair is refused.
3. **A write into either recorded range invalidates the recording**, and the next sample is
   refused.

Rule three is what makes the first two an argument rather than a hope, and it is the one an
obvious implementation leaves out. A shader that loads a second descriptor into the same eight
registers and samples again names the same registers both times - rule two sees one texture
where there are two, and the second sample reads the first one's texture. The frame looks
right. That is the exact failure D104 refused for export targets, arriving through the back
door.

Both refusals have a test that watches them happen. The rewrite test writes `s6`, in the middle
of `s[4:11]`, because a check watching only the first register of a group would miss it.

## 2. What the translation emits

Per lane: two registers read as floats and built into a coordinate, the bound sampled image
loaded, a sample at **level zero named explicitly**, then the components the mask selects
extracted and written to consecutive registers.

Level zero, explicitly, because that is what the `lz` in the guest's mnemonic says. Worklog 566
found that this is a different SPIR-V instruction from the implicit form rather than an option
on it, and the oracle can now emit either - so the translation is checked against a module doing
the same thing rather than a module doing something similar.

The mask's holes do not become holes in the destination: a mask selecting red, green and alpha
writes three consecutive registers, not four with a gap.

## 3. Two things the implementation had to be told

**The sample is refused at any stage but the fragment one**, and two independent reasons agree
on that. The harness binds its sampled image with fragment stage flags, so a pipeline whose
other stage sampled would be invalid. And the four-component vector type a sample answers with
is declared by the colour output, which only a fragment module has - a compute module would have
referenced a type nothing declared, which is a module a driver faults on rather than diagnoses.

**The dispatch arm went in the wrong match first.** There are two: the top-level one that
dispatches by name, and a memory sub-dispatch that looks almost identical at the point of
insertion. The instruction was listed as supported and then fell through to "no translation for
this instruction" - caught immediately by `the_supported_list_and_the_translator_agree`, which
exists for precisely this and had nothing else to say all session.

## 4. The list is empty for the second time

`BLOCKED` records instructions understood well enough to say what they are waiting on. It
emptied once at D553 and was kept rather than deleted, so the next instruction waiting on a
decision had somewhere to be recorded. Two went in after that; both have now come out, which is
what the list is for.

The texture sample's entry said the obstacle was "a descriptor this translator has no model for
and a host image view nothing here declares". Half of that was solved by building the host half
first. The other half was settled by deciding it did not need solving.

## 5. What the frame is and is not

It is a guest's instruction words - interpolate a coordinate, sample, export - translated here,
drawing four quadrants of a two-by-two texture in the right places, byte-identical to a
hand-written module doing the same thing.

It is **not** a claim that the texture is the one the guest asked for. It is the one the caller
bound, and which texture that is remains the caller's. What the translation guarantees is that a
module needing more than one says so at translation time instead of drawing.

Nor is it record B's frame. The record captured the command stream and the shader payload, not
the texture; the pixel hash remains a different claim.

## 6. Gates

The whole workspace passes, clippy is clean at `-D warnings`, formatting is clean, and every
device test runs clean under the Khronos validation layer - the translated sample included.

## 7. Files

- `crates/orbistoun-translate/src/model.rs` - `Texture`, `Model::sampled_image`, `image_sample`,
  the dispatch arm, `image_sample_lz` in `SUPPORTED`, and `BLOCKED` emptied.
- `crates/orbistoun-translate/src/wavefront.rs` - the lazily declared sampled image, the two
  refusals, and the scalar write that invalidates a recording.
- `crates/orbistoun-gpu-vulkan/tests/translated_sampling.rs` - the frame and both refusals.
- `crates/orbistoun-gpu/tests/oracle_gl_cube.rs` - record B's pixel shader now pinned as
  translating.

## Next

1. A window per buffer, or a window the pipeline derives from the submission: one span covers a
   vertex buffer or a canary, not both.
2. `REQ-20260914T1720Z-9c4a`, which settles the payload split D688 assumes.
3. The descriptor subsystem, whenever a shader arrives that needs two textures - D690 is
   written so that shader fails loudly rather than drawing.
