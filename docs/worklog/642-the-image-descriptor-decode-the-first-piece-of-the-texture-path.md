# 642. The image-descriptor (T#) decode: the first, measured piece of the texture path

**2026-09-16** - `decode_image_descriptor` reads a guest's texture dimensions, format and base out of
the eight-dword T#, the concrete counterpart of the sampled image the translator emits as arithmetic,
cited from the open-source driver - the part of the texture path that is not blocked on a measurement

## Why the decode and not the binding

The texture path's end - a guest's textured frame rendering through the executor - is blocked, and not
on code: a guest's texture pixels are block-compressed **and tiled**, and the tiling swizzle is
hardware nothing here has measured (G15). No amount of wiring reads tiled pixels correctly without it.

But the **descriptor** is not blocked. Like the vertex buffer's V# (worklog 634), a texture is
described by a T# the guest wrote - eight scalar dwords holding a base, an extent, a format and a
tiling mode - which the translator treats as opaque because the GPU reads it at run time
(`orbistoun_translate::model`, `IMAGE_DESCRIPTOR_REGISTERS`: "this translation reads none of them").
Decoding it into numbers is the frontend's first texture piece: it says what the texture *is* -
enough to size and format a host image - and waits on the tiling measurement to read its pixels.

## What was built, and how it is measured

`decode_image_descriptor([u32; 8]) -> ImageDescriptor { base, width, height, format }`, from the
GFX10 image resource descriptor. The layout is not from memory - it is transcribed from the
open-source driver's own construction and cited in the code: oops-mesa
`src/amd/registers/gfx10-rsrc.json` (`SQ_IMG_RSRC_WORD1`/`WORD2`) gives the bit positions, and
`src/amd/common/ac_descriptors.c:ac_build_gfx10_texture_descriptor` shows the encoding - the base in
dword 0 in 256-byte units plus dword 1's low byte, the format in dword 1 bits 28:20, and the extent
stored one below its size with **the width split**: its low two bits in dword 1 bits 31:30 and the
rest in dword 2 bits 11:0 (`S_00A008_WIDTH_HI((width - 1) >> 2)` against `S_00A004_WIDTH_LO(width -
1)`), the height in dword 2 bits 27:14. Following the driver's own register table is the same standard
the sibling mesa project holds itself to (oops-mesa principle 3), and stronger than a from-memory
offset - which is exactly the mistake the identity guard's register rule warns against.

## Made to fail

`an_image_descriptor_decodes_its_base_dimensions_and_format` builds the three dwords exactly as
`ac_descriptors.c` does and asserts the decode. The `1920 x 1080` case is the point: the width's low
two bits sit in one dword and the rest in another, so a decode reading the width from a single dword
gets it wrong. The small case carries a base with a high byte set, so both halves of the address are
exercised, and both cases would fail a decode that dropped the minus-one or masked the wrong bits.

## What is interim, named as such

- **The pixels wait on the tiling measurement (G15).** This decodes what the guest declared; reading
  its texels needs the tiling swizzle, which is a hardware measurement (an obSCEne probe), not code.
- **No accessor from memory yet.** A T# for the captured frame sits in a table in guest memory (the
  payload's `T#/S#` table), not in registers, and the flow that finds its address - a shader's
  user-data pointer - is not built, so this lands the decode primitive (like `decode_buffer_descriptor`
  did) and the read-from-memory accessor is a later piece.
- The executor's texture *binding* (routing the framebuffer's tested texture path through a command)
  is a separate piece and is not built here; it pays off only once a texture can be extracted, which
  waits on G15.

## Gate state

`cargo test --workspace` **2407 passed, 0 failed** (+1); `cargo clippy --workspace --all-targets -D
warnings` clean; fmt clean; identity scan exit 0. `ImageDescriptor` and `decode_image_descriptor`
re-exported from the crate root beside the other descriptor decodes. No commit.
