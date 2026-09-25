# 821. The rendered frame is written, not dropped — `REQ-...1f07` closed — and the first frames both fully-owned baselines produce are uniformly black

**2026-09-24** — worklog 820 got Neverball's first frame through the backend: 454 commands carried out,
none refused, a 1920x1080 frame. `render_and_log_last_submission` then threw the frame away, one line
before the route built to carry it (`REQ-...1f07`).

## The wire

- **`render::render_submission_to(submission, frames_dir)`** is the run path's whole step, taking the
  directory as an argument: it renders, logs, and writes the frame through
  `frame_region::write_frame`, returning the `Event::Frame` descriptor — or `None` with no directory,
  no device or no frame. `render_and_log_last_submission` takes the last submission and calls it with
  the run's frames directory, and now returns the descriptor.
- **The frames directory is the traces directory**, set by `render::frames_to` beside
  `report::trace_to` — the directory the shim's `describe` already reads a frame region from (D695).
- **Who emits the event.** The worker's protocol stream keeps exactly one writer by design (the request
  loop). The ordinary-return path is inside it, so it emits the `Event::Frame` with `write_message`. The
  time-limit and call-budget endings stop the process from their own thread; they write the region and
  log its name, and do not touch the stream.
- **The test is the run path's own**: `a_captured_submission_reaches_a_constructed_backend` now drives
  the console-triangle capture through `render_submission_to` with a temporary directory, checks the
  descriptor carries the frame's size, reads the bytes back by it and compares them with what `render`
  produced — and checks that no directory writes nothing. On the device: 5 commands, a 64x64 frame,
  `frame-1.bin`, read back intact.

`REQ-...1f07` is marked RESOLVED in the inbox.

## What the frames are

```
Neverball: 454 command(s) driven, 0 refused, frame 1920x1080 -> frame-1.bin, 8,294,400 bytes
cube:       16 command(s) driven, 0 refused, frame 1920x1080 -> frame-1.bin
both:       one colour at every sampled pixel - (0, 0, 0, 255)
```

The frame route works end to end, and it carries **nothing but opaque black** for both guests. Two
separate reasons stack here, and the census separates them only partly:

1. **The background.** The backend begins every frame from a cleared attachment — opaque black,
   `BACKEND_CLEAR` — where the console's target holds whatever the guest put there. Both guests clear
   their target with a `DMA_DATA` fill, which since D710 the command processor really performs into
   guest memory; the backend never reads it. That is `REQ-...77fa`: seed the attachment from the guest's
   target (detiled) when it lies in guest memory.
2. **The draws.** The cube's 12 draws are coloured faces, and not one pixel of the frame differs from
   the clear — so its draws, carried out without refusal, put nothing visible in the attachment either.
   That is not `77fa`; it is the next question once the background is right (geometry, viewport,
   export or attachment binding).

## Gate state

`crates/orbistoun-worker/src/render.rs` (`render_submission_to`, `frames_to`, the descriptor, the
test), `crates/orbistoun-worker/src/lib.rs` (the directory, the emitted event). The worker render tests
pass on the device. `./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No
commit.
