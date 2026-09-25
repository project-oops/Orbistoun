# 844. A performance overlay, and what it found

**2026-09-24**. Where a running title's time goes is now measured and shown.

- **`orbistoun_gpu::perf`:** atomic phase timers and counters shared by the command processor, the
  backend and the worker. The worker streams an `Event::Perf` once a second, from flips and from
  submissions. The GUI draws it over the picture (F3 toggles it), highlighting the biggest share. A
  headless run prints the same report.
- **What the numbers led to, in order:**
  - **A pipeline per submission** threw away the shader cache every time, so every shader was
    translated again: over half of every second. There is now one pipeline for the run.
  - A persistent pipeline names a module once, so the **backend now lasts the run** too. Modules
    whose submission was never drawn are carried forward to the next one.
  - **Draw state was found quadratically:** each draw rescanned every register write before it.
    `RegisterSweep` walks them once. Prepare went from ~280 to ~95 ms/s.
  - **A render pass per draw** loaded and stored the whole 1080p attachment around each of ~12,000
    draws a frame. Draws on one attachment are now recorded into one command buffer and one pass,
    submitted when something needs them (`settle`). A pipeline's one-time setup (texture upload,
    storage-image layout) happens outside the pass. Execute went from ~240 to ~8 ms/s.
  - **Other per-draw waste removed:** a command pool per draw, an attachment-sized storage-image
    copy per draw, the mesh function table looked up per draw, and the 64 KB window hashed per draw.
  - **The target is not re-read when unchanged:** if guest memory holds exactly what was last
    written back, the executor is handed `None` and draws on what it holds.
  - **Tiling is table-driven and parallel,** with the colour swap in the same pass.
  - **The cache key includes the scissor.** The scissor is fixed state in a pipeline, so leaving it
    out was a latent wrong-clip bug.
- **Checked:** cube frames 1, 5, 41 and 60 are byte-identical to the pre-optimisation baseline.

**Result:**

| | before | after |
|---|---|---|
| cube | ~1 fps | 11 fps |
| Neverball draws per second | ~6,000 | ~15,000 |

Neverball still flips about once a second. A frame is ~37 submissions (~15,000 draws), and each
submission still reads its 8 MB target back from the GPU, re-tiles it and writes it into guest
memory: ~15 of each submission's ~24 ms.
