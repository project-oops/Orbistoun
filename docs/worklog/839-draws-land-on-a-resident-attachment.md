# 839. Draws land on a resident attachment

**2026-09-24**. `framebuffer::ResidentAttachment` keeps each target's image on the device across a
submission's draws: seeded once (from the target's pixels, or cleared), drawn on in place, and read
back only when `VulkanBackend::last_frame` asks. Previously every draw created an attachment and
copied 8 MB in and 8 MB out.

- **Mesh draws** go through `draw_mesh_resident`.
- **The vertex path and `seed_target`** hand the target back to the host first (`retire_resident`).
- `last_frame` now takes `&mut self`.
- The old per-draw `draw_mesh_into` is gone.

**Results:**
- Neverball submissions: 1.2–1.4 s, from 2.5–2.8 s (and ~11 s two ticks ago).
- Cube: 38 of 38 submissions, 16 frames confirmed.

**Next wall:** Neverball's submission 31 never runs. Its pixel shader samples more than one texture,
which the translator refuses (D690): "a pipeline binds one". Multi-texture binding is next.
