# 809. `sceAgcDriverCreateQueue` hands the guest a queue handle, so the cube turns on its hardware pipeline and runs the real draw path — 150,996 calls, and the wall moves from "no hardware pipeline" to an unnamed submit hash

**2026-09-22** — the wall worklog 808 left: the cube reached `flipped` but drew nothing, printing
*no hardware pipeline: nothing is drawn*. The cause was in orbistoun's GPU driver, and it had been
written down as a deliberate omission waiting for exactly this guest.

## Why a null out-parameter turned the whole hardware path off

`sceAgcDriverCreateQueue(type, out_queue, flags)` returned the measured `0` but **left `*out_queue`
unwritten**, on the honest reasoning (D677) that obSCEne had measured the queue object's header but
not where the console places it, so fabricating a pointer was the plausible-output failure principle
3 forbids — and *"no guest reaches this call yet."*

The cube reaches it. Its GL context gates its **entire** hardware path on the handle being non-null:

```c
rc_q = sceAgcDriverCreateQueue(0u, &queue, 0u);
if (rc_q == 0 && queue != NULL) { /* build shaders, ctx->use_hardware = GL_TRUE ... */ }
```

A null `queue` skipped that block, so `use_hardware` stayed false, and every draw fell to
`gl_draw.c`'s `gl_hw_fail(ctx, "no hardware pipeline: nothing is drawn")`. The call was answered, not
stubbed — which is why worklog 808's run showed no unimplemented call behind the wall.

## The fix: hand the guest orbistoun's own opaque queue object

`create_queue` now writes a handle into `*out_queue`: the address of a single process-wide,
orbistoun-owned block (`queue_object`), filled with the header obSCEne measured
(`38 00 00 00 03 00 00 00 00 00 02 00`). This is **not** the fabricated hardware pointer principle 3
forbids — that would be inventing the console's placement. It is orbistoun's own handle at orbistoun's
own address, exactly as a video-out port handle or a file descriptor is: the guest holds it and passes
it back to a submit that reads the descriptor, never the queue, so the object is opaque here. The run
proved the opacity — the guest passed the handle (`arg0 = 0x261c34ce340`, the leaked block's own
address) straight to the submit call. The knowledge entry and a test (`create_queue_hands_back_a_non_null_handle`) record it.

## What the cube does now: the real hardware draw path

`use_hardware` goes true, and the run is transformed again:

```
[OOPS-GL] drawing straight into the scanout buffers (64KB_R_X)
[OOPS-GL] hardware AGC RDNA2 pipeline initialized successfully
```

The cube builds a real draw command buffer every frame and submits it — **150,996 import calls, up
from 32,988** (+118,008), because the hardware path does per-frame register writes, shader setup and
a submit where the software path did almost nothing. Verdict **FURTHER**. Reach stays `flipped`
(no pixels yet, see below). One consequence worth recording: the cube's *last submission is now a real
draw* rather than the flip's compute tiler, so `render_and_log_last_submission` drives a graphics
submission to the Vulkan backend, and the run takes two to four minutes rather than under one — the
backend is doing real work; a 20-second guest limit and a longer wall clock.

## Where it stops: an unnamed submit hash, and a dropped frame

- **`submit-rc: 0xf7ff0001`** → *the driver refused the command buffer*. The cube submits through
  `sceAgcDriverSubmitCommandBuffer`, whose NID `0x145f597e80e9876f` **resolves to no name in the
  symbol database**, so it lands on the loud-unimplemented stub and answers the placeholder. The GL
  context reads that as a refusal, fails the hardware clear self-test (the clear never ran), and marks
  the pipeline failed. Naming that hash and routing it to the same submission path as
  `sceAgcDriverSubmitDcb` is the next target — it is the gap between a built draw and a rendered one.
- **The rendered frame is still dropped** at `render.rs:94-97` (`render(&submission).outcome` keeps the
  outcome and discards `frame_bytes`) — which is inbox request `REQ-...1f07`, now genuinely
  exercisable because the cube produces a real submission for the backend to render.
- Two input calls (`scePadSetProcessPrivilege`, `scePadGetHandle`) remain stubbed once each,
  non-blocking as before.

## Gate state

`crates/orbistoun-gpu/src/agc_driver.rs` changes `create_queue` to write the handle and adds
`queue_object` and a test; `crates/orbistoun-hle/data/knowledge/libSceAgcDriver.toml` restates the
edge case. The write goes through the guest's own `void**` under the identity mapping, one `unsafe`
op with its `// SAFETY:`. The compat record advanced itself (`flipped - 21 imports, 150996 calls, 100%
standing`), so the generated docs are regenerated with it. `orbistoun-gpu` tests 91/… pass,
`./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
