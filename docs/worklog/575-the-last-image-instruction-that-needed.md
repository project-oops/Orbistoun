# 575. The last image instruction that needed no measurement, and the format it refuses to invent

**2026-09-15** - orbistoun-spirv, orbistoun-gpu-vulkan and orbistoun-translate, after worklog 573

`image_store` translates. The shader census stands at **183 of 184 instructions**, and the one
left is the one that needs a probe rather than code.

```
shaders      11 of 14 translate
instructions 183 of 184 translatable
  FURTHER  183 of 184 instructions (+1)
  cleared image_store
```

## 1. The question it was really blocked on

Not the write instruction, which is one opcode. **The format.**

A storage image in SPIR-V normally names its format in the module - `Rgba8`, `R32f` - and that
format decides how the bits a shader writes are interpreted. The guest's format is in the image
descriptor: eight scalar registers this project does not decode, and D690 is the decision not
to. So there was nothing to read it out of, and the only way to name one was to pick one.

Picking `Rgba8` would work on every texture that happens to be eight bits a channel and
**silently reinterpret every texture that is not**. The frame would render. That is the failure
this project is least able to detect.

D692 takes the other road: the module declares `Unknown` and the capability that permits it,
`StorageImageWriteWithoutFormat`, with the matching device feature requested where the device
offers it. `Unknown` is not a weaker claim than `Rgba8` - it is a **different kind of claim**. It
says the shader does not know the format, which is true, rather than that it is eight bits, which
is not known.

A device that does not offer the feature cannot run such a module. That is now reported before it
is asked for, so a caller sees a skip naming the reason rather than a pipeline failing to create.

## 2. Lazy for a stronger reason than the texture was

The sampled image is declared on first use so a module that never samples costs no descriptor.
The storage image has to be, because declaring it declares a **capability** - and a module
carrying a capability the device was not created with is refused rather than run. A module that
never stores must never carry it.

## 3. The host half, then the translation

The order that has worked four times now. First an image, a view, a binding at 3, the barrier
that makes it writable, the copy back out, and a hand-assembled module that writes one texel -
then the translation measured against it.

The oracle test asserts three things, and each rules out a different way of being wrong:

- the texel it named holds what it wrote - the store landing;
- the attachment holds the same colour - the module writes both, so a frame that came back as
  the clear would mean the shader never ran, which is otherwise indistinguishable from a shader
  that ran and stored nothing;
- **the corners of the image are not all that colour** - the image starts uninitialised and one
  texel is written, so an assertion that only looked at the written texel would pass against an
  image that happened to be full of it.

A draw now hands back three observations rather than two: the attachment, the guest-memory
window, and the storage image. A guest's shaders write all three.

## 4. What the translated store does with a partial mask

The components the mask does not select are written as **zero**, stated rather than inherited. A
guest storing three channels leaves the fourth to whatever the format says, and the format is the
thing this whole decision is about - so there is no value to preserve and no way to leave it
alone. Naming zero is the difference between a choice and an accident.

## 5. The gap this leaves open, on purpose

**A guest's load and store of the same descriptor are two host objects.** A sampled image is read
through binding 2 and a storage image written through binding 3, and nothing makes them the same
image. A shader that stored to a texture and then read it back would write one and read another.

Closing that means deciding whether every texture is bound twice, or whether a sampled read
becomes a storage read, and neither should be decided before something needs it. What exists is
enough for a shader that only writes. One that does both would be wrong in a way nothing here
catches, which is worth the sentence.

## 6. Files

- `crates/orbistoun-spirv/src/lib.rs` - `IMAGE_WRITE`, the capability, `NON_READABLE`,
  `STORAGE_IMAGE_BINDING`, `storing_fragment_module`, and a shape row.
- `crates/orbistoun-gpu-vulkan/src/compute.rs` - the device feature, requested and reported.
- `crates/orbistoun-gpu-vulkan/src/framebuffer.rs` - `StorageImage`, its barriers and readback,
  the binding, and `Drawn` carrying all three observations.
- `crates/orbistoun-translate/src/model.rs` - `Stored`, `Model::storage_image`, `image_store`.
- `crates/orbistoun-translate/src/wavefront.rs` - the lazily declared storage image.
- `crates/orbistoun-gpu-vulkan/tests/storage_image.rs` - the oracle and the translation.

## Next

1. `image_sample_l`, the last one. Its level comes from an address register whose position
   depends on dimensionality held in the descriptor - an operand order nobody here has measured,
   so it needs a probe rather than code.
2. `REQ-20260914T2348Z-4e71` and `REQ-20260914T1720Z-9c4a`, both on the obSCEne bus.
