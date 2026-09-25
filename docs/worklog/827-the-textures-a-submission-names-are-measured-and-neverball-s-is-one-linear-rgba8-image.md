# 827. The textures a submission's draws name are measured — Neverball's title screen names one, a 16x128 linear RGBA8 image, which needs no detiling to bind

**2026-09-24** — worklog 826 left Neverball's frame solid white: its draws cover the screen and every
texture sample reads the one white texel the backend binds by default. Before building texture
binding, the question that decides its shape: what textures do the draws actually name — which
formats, which tiling, which sizes? `detile_image` serves only the measured 64KB_R_X swizzle at 32 bpp
within one 64 KiB block, and a game's textures could need much more.

## The census

- **`SubmissionReport::textures`** (`orbistoun-gpu` `pipeline.rs`): every distinct image descriptor the
  draws' pixel shaders name, in first-use order, read the way the shader reads it — the fragment
  stage's user data words 0 and 1 are its descriptor table (worklog 825), and the table's first eight
  words are the T# (the GL context's textured shader loads them with
  `s_load_dwordx8 s[4:11], s[0:1], 0x00`), decoded with the existing `decode_image_descriptor`.
  `texture_census` skips a table that is not readable guest memory, which is every untextured draw's
  zeros.
- **The render log prints it** beside the command summary: size, format code, tiling, base.

The GL context gives each draw its own descriptor slot when a texture or its sampling state changes
(oops-sdk `gl_draw.c`, "the next slot, not a submission"), so a draw's table is its own and reading the
tables at submit time sees each draw's texture.

## What Neverball names

```
1 distinct texture(s) named:
  16x128 format 56 Linear at 0x740005bd0000
```

One texture across the title screen's draws: format 56 — `8_8_8_8_UNORM`, `gfx10-rsrc.json:61` as the
SDK's own `gl_pack_descriptors` cites it — **linear**. No swizzle to model, no multi-block layout, no
block compression: its texels are rows of RGBA8 in guest memory. The one layout detail is the row
pitch, which the SDK states: a 2D texture carries `pitch - 1` in `SQ_IMG_RSRC_WORD4` when its pitch
exceeds its width and zero (pitch = width) otherwise (`gl_state.c`, `gl_pack_descriptors`).

So binding a draw's own texture for Neverball is: decode the pitch, read `height` rows of `width`
texels at that pitch from the descriptor's base, and hand them to the backend per draw in place of the
default white texel — the next unit. The cube's depth test and culling (`REQ-...2ea9`) stay after it.

## Gate state

`crates/orbistoun-gpu/src/pipeline.rs` (`SubmissionReport::textures`, `texture_census`),
`crates/orbistoun-worker/src/render.rs` (the census lines). A measurement; the frames are as in worklog
826. `./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
