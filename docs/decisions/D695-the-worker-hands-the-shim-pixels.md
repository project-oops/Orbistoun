# D695 - The worker hands the shim pixels

**Status:** assumed
**Date:** 2026-09-15

The worker renders headless, with no window, surface or swapchain, reads each frame back to
ordinary bytes, and passes them in a shared region named by a small descriptor on the control
protocol; the shim uploads them as a texture.

**Why:** framebuffer comparison, the one cheap mechanical correctness check, needs a CPU
readback whatever the display path is, so readback as the display path adds no second path. It
constrains neither device creation, queue ownership nor image allocation, and external memory
can become an optimisation behind the same seam. The cost is a copy per frame, about 500 MB/s at
1080p and 60 Hz.

**Rejected:**
- A reparented child-owned window: platform-specific, impossible on some window systems, breaks
  headless runs, and cannot share a surface with the inspector.
- Shared images through external memory: constrains device creation from the first line and
  still needs the readback.
