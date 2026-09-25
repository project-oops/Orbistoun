# 830. Neverball's first frame is its space background, rendered correctly — the one colour is the asset's own, and the wall moves from rendering to the frame after it

**2026-09-24** — worklog 829 found that Neverball's uniform (0, 0, 25) frame survives blending being
applied, and guessed that every fragment samples one texel of its texture. That guess needed a check of
the texture itself before anything else.

## The check

The render log's command summary now describes a `BindTexture` by how varied its texels are and its two
ends, rather than only its size. Neverball's:

```
BindTexture { 16x128, 1 distinct texels, first 0xff190000, last 0xff190000 }
```

**The texture is one colour**: every one of its 2,048 texels is `0xff190000` — in memory order R 0,
G 0, B 25 (0x19), A 255. The frame's (0, 0, 25) is exactly what the texture holds, so sampling is not
at fault.

## What the texture is

Neverball's `back/*.png` are all 16x128 — the gradients its sky sphere wraps around a scene — and the
run opened `back/space.png`, the title screen's. Decoded from the game's own data (the upstream
`data/back/space.png`, 8-bit RGB): **one distinct pixel, (0, 0, 25), in every row**. The title's space
background is a uniform dark blue.

So Neverball's first submission — 450 draws, one texture — is the background sphere, and the frame
orbistoun renders from it, every pixel (0, 0, 25), is **that frame drawn correctly**: the asset's own
colour, sampled from the guest's own texels, through the guest's own shaders, with its blend state. The
logo, the menu text and everything else the title screen shows are drawn in the frames after it.

## Where that leaves the wall

Neverball never reaches a second frame. After its first submission it polls the fence that submission's
closing `RELEASE_MEM` writes, and D705/D710 let a fence be written only when the work before it has
really run: the command processor's memory work does run at submit (D710), but the draws run only after
the guest has stopped (the render step, worklogs 814-829). The draws now render correctly — so
**executing a submission's draws at submit**, and letting the fence after them retire from work that
ran, is what the next frame waits on. That is the rendering path's last piece and the next unit.

## Gate state

`crates/orbistoun-worker/src/render.rs` (the `BindTexture` summary line). A diagnostic; the frames are
as in worklog 829. `./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No
commit.
