# 847. A window ring, and the GPU's own clock

**2026-09-24**. The per-submission wait for the device is gone, and the overlay now says how busy
the GPU is.

- **Window ring:** the guest-memory window is three slots in one buffer. A changed window goes into
  the next slot, waiting only on that slot's own fences, which have normally signalled already.
  Binding 1 is a `STORAGE_BUFFER_DYNAMIC`, so a cached pipeline's set never changes: each draw's
  slot is a dynamic offset at bind. `DispatchBuffer` gained an `offset`, which the compute path and
  `read_buffer` honour. Before, the window was rewritten in place after a full device idle on every
  submission.
- **GPU clock:** timestamp queries bracket every pass of resident draws and are read at `settle`.
  They report as `gpu busy`, a phase that runs alongside the host's, and the overlay shows it as its
  own line.
- **Checked:** cube frames 1, 5, 41 and 60 match the baseline except for the HUD's timing text.

**What the clock showed:**

- With no waits left, Neverball submits a whole frame at a time (12-26k commands). The host's own
  work is ~250 ms/s, and the **GPU is busy 800-850 ms/s** - about 270 ms per frame, ~15 us per
  draw. Neverball is **GPU-bound at ~3 fps**.
- The cause is the translated shaders. A Neverball pixel shader is **18,880 SPIR-V instructions**:
  4,841 `OpAccessChain`, 3,855 loads, and ~1,100 each of predication `Select`s and mask tests. The
  emulated register file lives in arrays and every write is predicated on an emulated exec mask.
  The guest program is a few dozen instructions.
- The next lever is the translator's code generation, not the executor.
