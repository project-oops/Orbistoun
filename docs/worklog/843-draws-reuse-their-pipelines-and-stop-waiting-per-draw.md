# 843. Draws reuse their pipelines and stop waiting per draw

**2026-09-24**. Titles that run at full speed on hardware ran at about 1 fps: a 450-draw GL1 Cube
submission took 1.34 s.

A per-phase timer (`orbistoun_gpu_vulkan::timing`, printed on each submission's line) showed where
the time went, per draw:

| phase | per draw |
|---|---|
| build pipeline | ~1.3 ms |
| release pipeline | ~1.2 ms |
| submit + wait for idle | ~0.3 ms |
| read back | ~0.2 ms |

- **Pipeline cache:** resident draws keep their pipelines in a process-wide cache.
  - The key hashes both shaders, both textures, the blend, the viewport transform and the extent.
  - Shader hashes are taken once, when a module becomes resident. Texture hashes are taken once,
    at `BindTexture`.
  - On reuse, binding 1 is re-pointed at the current guest-memory window, which is recreated each
    submission, and the textures are not copied again.
  - Shaders and textures are now shared `Arc`s rather than copied into every draw.
  - The cache is capped at 512 pipelines.
- **No per-draw readback:** the guest-memory window is read once, when something asks for it
  (`last_window`). The storage image was never used on this path, so it isn't read at all.
- **No per-draw wait:** resident draws are submitted without waiting. Each draw opens with a barrier
  that orders it after the one before. The host settles (`framebuffer::settle`) only where it
  touches device memory: reading a target or buffer back, replacing the window, updating or
  releasing a cached pipeline, compute dispatches and the vertex path.
- **Latest-frame file:** the 8 MB `latest-drawn-frame.rgba` is written at most once a second, not
  every submission.

**Result:**

| | before | after |
|---|---|---|
| cube, 450-draw submission | 1,338 ms | 28 ms |
| cube, 40 s run | ~38 submissions | 633 submissions |
| Neverball, per submission | ~1,450 ms | ~25 ms of draws |
| Neverball, 60 s run | 101 submissions in 150 s | 786 submissions |

Cube frames 1, 5, 41 and 60 are byte-identical with the cache on and with it off.

What's left per submission, measured on Neverball: reading and detiling the 8 MB target (~9 ms), the
executor's own work outside the draws (~10 ms), and re-tiling and writing back (~5 ms).
