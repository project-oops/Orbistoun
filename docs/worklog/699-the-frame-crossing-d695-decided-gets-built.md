# 699. The frame crossing D695 decided gets built

**2026-09-19** — inbox `-7f1b`: D695 chose how a rendered frame reaches a person - bytes in a shared
region named by a small descriptor, never pixels in a message (D035) - and built nothing. The backend
correctly refuses `present`, so this is the only route a frame has, and until it exists a frame
produced by 36c0 would have nowhere to go. Three pieces, all built.

## The route

- **The descriptor (proto).** A new `Event::Frame { width, height, format, sequence, region }` on the
  worker→shim protocol. `region` is a bare name, never a pointer or a handle - the bytes cross as bulk
  in the region the worker wrote, and the message carries only how to find and read them (D035). A new
  `FrameFormat` enum (`Rgba8` today, the 32-bpp the detile produces) so a second layout is a variant
  the shim is made to handle rather than a silent reinterpretation. `PROTOCOL_VERSION` 3 → 4, since a
  peer without the variant would reject the message - the incompatibility the counter exists to catch.
- **The writer (worker).** A `frame_region` module: `write_frame(dir, seq, w, h, format, bytes)`
  writes the bytes to `frame-<seq>.bin` in the shared directory and returns the descriptor. Its
  inverse `read_frame(dir, event)` reads them back. Write and read live together because the region's
  layout is defined once, by the side that writes it - two places computing the name or length
  differently is how a reader comes to disagree with the writer (D084), so the shim reads through this
  same code rather than reimplementing it.
- **The reader (gui).** `frame::frame_image(dir, event)` reads the region through the worker's
  `read_frame` and turns the bytes into an `egui::ColorImage` ready for `load_texture`, refusing a
  region whose byte count is not what the dimensions and format require. Wired into the run's event
  rendering, reading from `traces_dir` - the shared route the traces already take, which is exactly
  what D695 meant by "the route bulk already takes". The image goes to an actual texture once a run
  produces a frame to send (36c0); the crossing itself is built and exercised now.

## Made to fail

- **The round trip** (`orbistoun-worker`): the worker writes a known 4 KB buffer, the shim reads it
  back byte-for-byte - and then the region is overwritten and the read no longer matches, which is the
  load-bearing half: it proves the reader reads the file rather than echoing the descriptor.
- **The shim end** (`orbistoun-gui`): a 2×1 frame the worker wrote reads into a `ColorImage` of the
  right size with its two pixels intact and in order; a region three bytes short of a 1×1 RGBA frame
  is refused rather than uploaded half (`from_rgba_unmultiplied` would have panicked on it).
- **D035, structurally** (`orbistoun-proto`): the serialised `Frame` names its region and contains no
  `0x`, `ptr`, `handle` or `addr` - a message carries the bulk's name, never a reference to it.
- **A traversal guard**: a `region` with a separator or a parent component is refused before it names
  a file the frame had no business naming - the descriptor is data, and this one may arrive over the
  wire.

## Gate state

`./bin/orbistoun check` passes end-to-end (all checks passed): `orbistoun-proto`/`-worker`/`-gui`
clippy `-D warnings` clean, tests pass (proto 13, and the new frame tests in worker and gui), prose
exit 0, `status --check` exit 0, doc gate clean, identity scan clean. Corpus unchanged. No commit.
No decision file: this builds D695's already-decided route, and the file-based region is the "simplest
thing that works" D695 explicitly left to implementation, not a new concept.
