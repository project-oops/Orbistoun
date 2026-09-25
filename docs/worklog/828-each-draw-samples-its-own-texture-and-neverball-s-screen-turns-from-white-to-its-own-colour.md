# 828. Each draw samples the guest's own texture — Neverball's screen goes from the default white to a colour of its own, and the next wall is the blend state no draw applies

**2026-09-24** — worklog 827 measured the one texture Neverball's title screen names: 16x128,
`8_8_8_8_UNORM`, linear. This binds it.

## What was built

- **`RenderCommand::BindTexture { texels, width, height }`** (`orbistoun-gpu` `backend.rs`): the texture
  the draws that follow sample, as tightly packed texels in the guest's own byte order.
- **The pipeline emits it** (`bind_textures`, on the live path that feeds user data): after each fragment
  `SetUserData`, `read_texture` reads the descriptor table the words name and, when the T# is one read
  exactly — `TYPE` 2D (9, Mesa `ac_descriptors.c:372` as the SDK cites it; obSCEne `-6c80` measured
  `0x90000fac` for its 2D control), linear, `8_8_8_8_UNORM` (56) — reads its texels and inserts the
  command. Anything else inserts nothing and the backend's default texel stays bound.
- **The pitch is the descriptor's.** `ADDR_SW_LINEAR` aligns a row to 256 bytes (Mesa
  `gfx9addrlib.cpp:5117-5127`), so a 16-texel-wide texture's rows are 64 texels apart, and a 2D T#
  carries `pitch - 1` in `SQ_IMG_RSRC_WORD4` bits 0-13 when the pitch exceeds the width
  (`gfx10-rsrc.json:401-406`); read at the width, every row after the first would come from the wrong
  place. Test `a_linear_texture_is_read_at_its_descriptors_pitch`: a 16x2 texture with pitch 64 — row 1
  comes from texel 64, not the marker planted at texel 16 — and a 3D descriptor binds nothing.
- **`VulkanBackend`** keeps the bound texture and hands it to each draw with its row length (the draw's
  `Start` gains it, falling back to the default texel); a `BindTexture` whose texel count disagrees with
  its size is a device error rather than a guess. The render log's command summary prints a texture as
  its size, not its texels.

## What Neverball draws now

```
BindTexture { 16x128 }
906 command(s) driven, 0 refused, 1920x1080
every one of 2,073,600 pixels (0, 0, 25, 255)
```

The draws sample the guest's texture — the frame is no longer the default texel's white but a colour
from the guest's own texels, a very dark blue. It is still one colour everywhere: whichever draw covers
the screen last paints all of it. Neverball fades its screens in and out with a translucent full-screen
quad, and the frame reads exactly like one drawn **opaque** — blending is decoded (`blend_control`, one of
the fields `REQ-...2ea9` lists as reaching the backend with nothing reading it) and never applied, the
same way the cube's depth test and back-face culling are (worklog 826). `2ea9` is therefore the next wall
for both fully-owned baselines.

## Gate state

`crates/orbistoun-gpu/src/backend.rs` (`BindTexture`), `pipeline.rs` (`bind_textures`, `read_texture`,
`IMAGE_TYPE_2D`, `FORMAT_8_8_8_8_UNORM`, the test); `crates/orbistoun-gpu-vulkan/src/framebuffer.rs`
(`Start`'s texture, both entry points), `lib.rs` (the bound texture, the arm, the name);
`crates/orbistoun-worker/src/render.rs` (the summary line). All 32 test binaries of `orbistoun-gpu`,
`orbistoun-gpu-vulkan` and `orbistoun-worker` pass. `./bin/orbistoun check` green, worklog index
regenerated, identity scan clean. No commit.
