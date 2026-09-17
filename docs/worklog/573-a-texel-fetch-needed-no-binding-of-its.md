# 573. A texel fetch needed no binding of its own, and the store still does

**2026-09-15** - orbistoun-spirv, orbistoun-translate and orbistoun-gpu-vulkan, after worklog 572

Three MIMG instructions were left. Worklog 572 said what each was waiting on, and one of those
answers turned out to be wrong in a useful direction.

It said `image_load` and `image_store` both "use an image with no sampler, which is a storage
image and a different binding - host work before it is translation". **That is true of the store
and not of the load**, and the difference is worth the paragraph it takes to say.

## 1. A load is a fetch, and a fetch takes an image

A guest's `image_load` reads one texel by its integer index. There is nothing to filter, nothing
to wrap, and no level to choose. On the host that is `OpImageFetch`, which takes an **image** -
and an image is what `OpImage` gets out of the sampled image the pipeline already binds.

So a load needs:

- no descriptor of its own,
- no device feature,
- and **no claim about the texture's format**, which is the one that mattered. A storage image
  declares its format in the module, and the guest's format lives in a descriptor this
  translator does not decode (D690). Claiming one would be a frame that renders and is subtly
  wrong, which is the failure this project is least able to detect.

A store is a different matter: nothing writes to a sampled image. That one genuinely needs a
storage image, a binding, the write-without-format capability, and a decision to go with it.

## 2. The numbers

Measured with `orbistoun-cli shaders`, before and after:

| | before | after |
|---|---|---|
| instructions translatable | 180 of 184 | **182 of 184** |
| shaders translating | 11 of 14 | 11 of 14 |

The shader count does not move because the one fixture that loads also stores. That is the
honest shape of a partial answer and the reason the instruction count is the number worth
reading here.

## 3. The encodings, measured

`texelFetch` compiled and disassembled, like every other encoding here:
`OpImageFetch` = 95, carrying a literal operand mask and then the level - the same shape as the
explicit sampling form; `OpImage` = 100. Vulkan requires the level be named for an image that is
not multi-sampled, so in practice a fetch always carries one.

Two shape-table rows added, and the reasoning is the same as for the sampling pair: the mask at
index four is a literal, and the identifiers it announces start at five.

## 4. What the guest's operands do that is awkward

**Index three is not the same field in both instructions.** A sample names a sampler there; a
fetch has no sampler, so its component mask sits where the sampler would be. The solved layouts
agree - five operands against four - and the mnemonic is what decides which to read.

That is now in one place. Reading the operands and checking them moved into its own function,
which the line limit asked for and which reads better anyway: one function says what the
instruction says, another says what to emit.

## 5. D690's rules, with a sampler that may not exist

The recorded sampler became optional, and the three rules needed one adjustment each:

- A sampler conflicts only with **another sampler**. A fetch names none, so it neither conflicts
  with the recorded one nor clears it - otherwise a module that fetched and sampled the same
  image would be refused for using two of something it uses one of.
- The first instruction to name a sampler is what records one, which may be later than the first
  to name the image.
- The write that invalidates a recording checks the sampler's range only when a sampler was
  named.

Fetching and sampling the **same** image descriptor is one texture and is allowed. Two different
image descriptors is still refused, whichever instructions name them.

## 6. The device test, and why it uses constants

Four draws, one per texel of a two-by-two texture, each asserting every pixel is that texel. The
coordinate is put in the registers by `v_mov_b32` rather than interpolated, deliberately: a fetch
takes a texel's **index**, so the claim worth making is "this texel, exactly". An interpolated
coordinate would make it "some texel, depending where the pixel is", which is a weaker claim
about a harder thing.

Four indices give four answers, so a fetch ignoring its coordinate fails rather than passing by
luck, and the two rows are told apart - the same upload-order claim a sample makes.

Clean under the Khronos validation layer.

## 7. Files

- `crates/orbistoun-spirv/src/lib.rs` - `IMAGE_FETCH`, `IMAGE`, and their shape rows.
- `crates/orbistoun-translate/src/model.rs` - `ImageAccess` and `image_access`, the fetch branch,
  the image and texel-coordinate types on `Texture`, and `image_load` in `SUPPORTED`.
- `crates/orbistoun-translate/src/wavefront.rs` - the image type exposed, the integer coordinate
  declared, and an optional sampler through all three of D690's rules.
- `crates/orbistoun-gpu-vulkan/tests/translated_sampling.rs` - the fetch, four texels.

## Next

1. `image_store`, which needs a storage image, the write-without-format capability, and a
   decision recording why the format is not claimed.
2. `image_sample_l`, which needs a probe: its level comes from an address register whose
   position depends on dimensionality held in the descriptor.
3. `REQ-20260914T2348Z-4e71` and `REQ-20260914T1720Z-9c4a`, both on the obSCEne bus.
