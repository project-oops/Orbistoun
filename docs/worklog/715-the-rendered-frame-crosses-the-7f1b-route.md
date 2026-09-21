# 715. The rendered frame crosses the 7f1b route, end to end

**2026-09-19** — a follow-up to `-36c0` (worklog 714): that wired the worker to drive a real
submission to a constructed backend, and noted the produced frame was dropped. This connects the
two halves D695 named - "the worker renders headless **and hands the shim the bytes**" - so a
rendered frame's bytes cross the frame route (7f1b) intact.

## What changed

`render` now returns a `Rendered { outcome, frame_bytes }`: the small summary kept apart from the
frame's bulk, so a caller that only wants to know a command reached the backend does not carry a
frame it will not read. The frame's bytes come straight from the backend's `last_frame()`.

The device test that drives the console triangle's command buffer now goes one step further: it
takes the frame `render` produced, writes it into a region with `frame_region::write_frame` (the
7f1b transport), reads it back by the returned `Event::Frame` descriptor with `read_frame`, and
asserts the bytes crossed intact. So the two seams - `render` (`-36c0`) and the frame route (7f1b) -
are shown meeting, on a real rendered frame, not asserted to.

## What is deliberately still open

`render_and_log_last_submission` - the run-path entry - still drops the frame's bytes rather than
writing them into the run's frames directory and emitting the `Event::Frame` that would carry them
to a running shim. That last wire needs the frames directory threaded to the render point, and it
is forward-looking: no corpus title reaches a submission yet (worklog 714), so nothing would be
written on a real run today. It is one directory-threading step, deferred until a title submits
rather than plumbed blind - the same reason the fault and time-limit paths wait. The pipeline's
pieces are connected and tested; the last wire is the run actually feeding them.

## Gate state

`./bin/orbistoun check` passes end-to-end (all checks passed): workspace compiles, clippy
`-D warnings` clean, the render tests (including the frame round-trip) and the device triangle/point
renders pass, docs/prose/fmt clean, identity scan clean. No commit.
